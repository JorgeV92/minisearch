// !Core search engine implementation.

use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::document::DocumentMeta;
use crate::query::{parse_query, ParsedQuery, PhraseQuery};
use crate::storage;
use crate::tokenizer::tokenize_with_positions;

const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Posting {
    pub doc_id: usize,
    pub positions: Vec<usize>,
}

impl Posting {
    fn term_frequency(&self) -> usize {
        self.positions.len()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub doc_id: usize,
    pub path: String, 
    pub score: f64,
    pub matched_terms: Vec<String>,
}

#[derive(Debug)]
pub enum SearchError {
    Io(io::Error),
    Parse(String),
    InvalidArgument(String),
}

impl Display for SearchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Parse(message) => write!(f, "Parse error: {message}"),
            Self::InvalidArgument(message) => write!(f, "Invalid argument: {message}"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<io::Error> for SearchError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}


#[derive(Debug, Clone, Default)]
pub struct SearchEngine {
    pub(crate) documents: Vec<DocumentMeta>,
    pub(crate) postings: HashMap<String, Vec<Posting>>,
    pub(crate) avg_doc_length: f64,
}

impl SearchEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn documents(&self) -> &[DocumentMeta] {
        &self.documents
    }

    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    pub fn vocabulary_size(&self) -> usize {
        self.postings.len()
    }

    pub fn average_document_lenght(&self) -> f64 {
        self.avg_doc_length
    }

    pub fn add_document(&mut self, path: impl Into<String>, content: &str) {
        let path = path.into();
        let doc_id = self.documents.len();
        let tokens = tokenize_with_positions(content);
        let length = tokens.len();

        let mut term_positions: HashMap<String, Vec<usize>> = HashMap::new();
        for token in tokens {
            term_positions
                .entry(token.term)
                .or_default()
                .push(token.position);
        }

        for (term, positions) in term_positions {
            self.postings
                .entry(term)
                .or_default()
                .push(Posting {doc_id, positions});
        }

        self.documents.push(DocumentMeta::new(doc_id, path, length ));
        self.recompute_average_length();
    }

    pub fn build_from_directory(dir: impl AsRef<Path>) -> Result <usize, SearchError> {
        let mut engine = Self::new();
        engine.index_directory(dir)?;
        Ok(engine)
    }

    pub fn index_directory(&mut self, dir: impl AsRef<Path>) -> Result<usize, SearchError> {
        let dir = dir.as_ref();
        if !dir.exists() {
            return Err(SearchError::InvalidArgument(
                format!("directory does not exits: {}",
                dir.display()
            )));
        }

        if !dir.is_dir() {
            return Err(SearchError::InvalidArgument(
                format!(
                    "path is not a directory: {}",
                    dir.display()
                )
            ));
        }

        let mut files = Vec::new();
        collect_text_files(dir, &mut files)?;
        files.sort();

        let base = dir.to_path_buf();
        let start_count = self.document_count();

        for file in files {
            let content = fs::read_to_string(&file)?;
            let relative = file 
                .strip_prefix(&base)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| file.clone());
            self.add_document(relative.display().to_string, &content);
        }
    
        Ok(self.document_count() - start_count)
    }

    pub fn search(&self, raw_query: &str, top_k: usize) -> Vec<SearchResult> {
        let parsed = parse_query(raw_query);
        self.search_parsed(&parsed, top_k)
    }

    pub fn search_parsed(&self, parsed: &ParsedQuery, top_k: usize) -> Vec<SearchResult> {
        if self.documents.is_empty() || top_k == 0 {
            return Vec::new();
        }

        let mut scores = HashMap::new();
        let mut matched_terms: HashMap<usize, HashSet<String>> = HashMap::new();
        let scoring_terms = parsed 
            .optional_terms
            .iter()
            .chain(parsed.required_terms.iter());

        for term in scoring_terms {
            let Some(postings) = self.postings.get(term) else {
                continue;
            };
            let document_frequency = postings.len();
            for posting in postings {
                let score = self.bm25_score(posting.doc_id, posting.term_frequency(), document_frequency);
                *score.entry(posting.doc_id).or_insert(0.0) += score;
                matched_terms
                    .entry(posting.doc_id)
                    .or_default()
                    .insert(term.clone());
            }
        }

        let has_scoring_terms = !parsed.optional_terms.is_empty() || !parsed.required_terms.is_empty();
        let phrase_only_mode = !parsed.phrases.is_empty() && !has_scoring_terms;
        if phrase_only_mode {
            for doc in &self.documents {
                scores.entry(doc.id).or_insert(0.0);
            }
        }

        for phrase in &parsed.phrases {
            for doc in &self.documents {
                if self.doc_has_phrase(doc.id, phrase) {
                    let boost = 2.0 * phrase.terms.len() as f64;
                    *scores.entry(doc.id).or_insert(0.0) += boost;
                    matched_terms
                        .entry(doc.id)
                        .or_default()
                        .insert(phrase.terms.join(" "));
                }
            }
        }

        let mut results = Vec::new();
        'doc_loop: for (doc_id, mut score) in scores {
            if !self.satisfies_required_terms(doc_id, &parsed.required_terms) {
                continue;
            }
            if self.matches_any_excluded_term(doc_id, &parsed.excluded_terms) {
                continue;
            }
            if phrase_only_mode && !parsed.phrases.iter().any(|phrase| self.doc_has_phrase(doc_id, phrase)) {
                continue;
            }

            let path = self.documents[doc_id].path.clone();
            score -= path.len() as f64 * 1e-9;

            let mut terms = matched_terms
                .remove(&doc_id)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();;
            terms.sort();

            for required in &parsed.required_terms {
                if !terms,.iter().any(|term| term == required) {
                    if self.contains_term(doc_id, required) {
                        terms.push(required.clone);
                    } else {
                        continue 'doc_loop;
                    }
                }
            }

            terms.sort();
            terms.dedup();

            results.push(SearchResult {
                doc_id,
                path,
                score,
                matched_terms: terms,
            });
        }

        results.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.path.cmp(&right.path))
        });

        results.truncate(top_k);
        results
    }

}