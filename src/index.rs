//! Core search engine implementation.

use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::document::{self, DocumentMeta};
use crate::query::{parse_query, ParsedQuery, PhraseQuery};
use crate::storage;
use crate::tokenizer::tokenize_with_positions;

const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;

/// A positional posting for a term in one document.
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

/// One search result returned by the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub doc_id: usize,
    pub path: String,
    pub score: f64,
    pub matched_terms: Vec<String>,
}

/// Error type used by indexing, searching, and persistence APIs.
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

/// A small in-memory search engine backed by an inverted index.
#[derive(Debug, Clone, Default)]
pub struct SearchEngine {
    pub(crate) documents: Vec<DocumentMeta>,
    pub(crate) postings: HashMap<String, Vec<Posting>>,
    pub(crate) avg_doc_length: f64,
}

impl SearchEngine {
    /// Creates an empty engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns document metadata for all indexed documents.
    pub fn documents(&self) -> &[DocumentMeta] {
        &self.documents
    }

    /// Returns the number of indexed documents.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Returns the number of unique terms in the lexicon.
    pub fn vocabulary_size(&self) -> usize {
        self.postings.len()
    }

    /// Returns the average document length in normalized tokens.
    pub fn average_document_length(&self) -> f64 {
        self.avg_doc_length
    }

    /// Adds one document to the engine.
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
                .push(Posting { doc_id, positions });
        }

        self.documents.push(DocumentMeta::new(doc_id, path, length));
        self.recompute_average_length();
    }

    /// Builds an engine from a directory of `.txt` and `.md` files.
    pub fn build_from_directory(dir: impl AsRef<Path>) -> Result<Self, SearchError> {
        let mut engine = Self::new();
        engine.index_directory(dir)?;
        Ok(engine)
    }

    /// Indexes all `.txt` and `.md` files under a directory recursively.
    pub fn index_directory(&mut self, dir: impl AsRef<Path>) -> Result<usize, SearchError> {
        let dir = dir.as_ref();
        if !dir.exists() {
            return Err(SearchError::InvalidArgument(format!(
                "directory does not exist: {}",
                dir.display()
            )));
        }
        if !dir.is_dir() {
            return Err(SearchError::InvalidArgument(format!(
                "path is not a directory: {}",
                dir.display()
            )));
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
            self.add_document(relative.display().to_string(), &content);
        }

        Ok(self.document_count() - start_count)
    }

    /// Searches the index using a raw query string.
    pub fn search(&self, raw_query: &str, top_k: usize) -> Vec<SearchResult> {
        let parsed = parse_query(raw_query);
        self.search_parsed(&parsed, top_k)
    }

    /// Searches the index using a pre-parsed query.
    pub fn search_parsed(&self, parsed: &ParsedQuery, top_k: usize) -> Vec<SearchResult> {
        if self.documents.is_empty() || top_k == 0 {
            return Vec::new();
        }

        let mut scores: HashMap<usize, f64> = HashMap::new();
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
                *scores.entry(posting.doc_id).or_insert(0.0) += score;
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

            // Tiny tie-breaker favoring shorter paths for equal scores.
            let path = self.documents[doc_id].path.clone();
            score -= path.len() as f64 * 1e-9;

            let mut terms = matched_terms
                .remove(&doc_id)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            terms.sort();

            for required in &parsed.required_terms {
                if !terms.iter().any(|term| term == required) {
                    // Required term matched but may not have been inserted in phrase-only situations.
                    if self.contains_term(doc_id, required) {
                        terms.push(required.clone());
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

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), SearchError> {
        storage::save_engine(self, path.as_ref())
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, SearchError> {
        storage::load_engine(path.as_ref())
    }

    pub(crate) fn postings(&self) -> &HashMap<String, Vec<Posting>> {
        &self.postings
    }

    pub(crate) fn from_parts(
        documents: Vec<DocumentMeta>,
        postings: HashMap<String, Vec<Posting>>,
        avg_doc_length: f64,
    ) -> Self {
        Self {
            documents,
            postings,
            avg_doc_length,
        }
    }

    fn recompute_average_length(&mut self) {
        if self.documents.is_empty() {
            self.avg_doc_length = 0.0;
            return;
        }
        let total_length: usize = self.documents.iter().map(|doc| doc.length).sum();
        self.avg_doc_length = total_length as f64 / self.documents.len() as f64;
    }

    fn bm25_score(&self, doc_id: usize, term_frequency: usize, document_frequency: usize) -> f64 {
        let total_docs = self.documents.len() as f64;
        if total_docs == 0.0 || term_frequency == 0 {
            return 0.0;
        }

        let doc_length = self.documents[doc_id].length as f64;
        let avg_length = if self.avg_doc_length > 0.0 {
            self.avg_doc_length
        } else {
            1.0
        };
        let tf = term_frequency as f64;
        let df = document_frequency as f64;

        let idf = ((total_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
        let numerator = tf * (BM25_K1 + 1.0);
        let denominator = tf + BM25_K1 * (1.0 - BM25_B + BM25_B * (doc_length / avg_length));
        idf * (numerator / denominator)
    }

    fn satisfies_required_terms(&self, doc_id: usize, required_terms: &[String]) -> bool {
        required_terms.iter().all(|term| self.contains_term(doc_id, term))
    }

    fn matches_any_excluded_term(&self, doc_id: usize, excluded_terms: &[String]) -> bool {
        excluded_terms.iter().any(|term| self.contains_term(doc_id, term))
    }

    fn contains_term(&self, doc_id: usize, term: &str) -> bool {
        self.postings
            .get(term)
            .map(|postings| postings.iter().any(|posting| posting.doc_id == doc_id))
            .unwrap_or(false)
    }

    fn positions_for_term_in_doc(&self, term: &str, doc_id: usize) -> Option<&[usize]> {
        self.postings.get(term).and_then(|postings| {
            postings
                .iter()
                .find(|posting| posting.doc_id == doc_id)
                .map(|posting| posting.positions.as_slice())
        })
    }

    fn doc_has_phrase(&self, doc_id: usize, phrase: &PhraseQuery) -> bool {
        if phrase.terms.is_empty() {
            return false;
        }
        if phrase.terms.len() == 1 {
            return self.contains_term(doc_id, &phrase.terms[0]);
        }

        let Some(first_positions) = self.positions_for_term_in_doc(&phrase.terms[0], doc_id) else {
            return false;
        };

        let mut lookup_sets: Vec<HashSet<usize>> = Vec::with_capacity(phrase.terms.len() - 1);
        for term in phrase.terms.iter().skip(1) {
            let Some(positions) = self.positions_for_term_in_doc(term, doc_id) else {
                return false;
            };
            lookup_sets.push(positions.iter().copied().collect());
        }

        'outer: for start in first_positions {
            for (offset, set) in lookup_sets.iter().enumerate() {
                if !set.contains(&(start + offset + 1)) {
                    continue 'outer;
                }
            }
            return true;
        }

        false
    }
}

fn collect_text_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), SearchError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_text_files(&path, files)?;
        } else if is_supported_text_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_supported_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("txt") | Some("md")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_prefers_document_with_more_matches() {
        let mut engine = SearchEngine::new();
        engine.add_document("doc1.txt", "rust search engine rust bm25");
        engine.add_document("doc2.txt", "rust engine");

        let results = engine.search("rust", 10);
        assert_eq!(results[0].path, "doc1.txt");
    }

    #[test]
    fn phrase_query_matches_exact_sequence() {
        let mut engine = SearchEngine::new();
        engine.add_document("a.txt", "distributed systems are fun");
        engine.add_document("b.txt", "systems distributed are mentioned");

        let results = engine.search("\"distributed systems\"", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "a.txt");
    }

    #[test]
    fn required_and_excluded_terms_filter_results() {
        let mut engine = SearchEngine::new();
        engine.add_document("rust.txt", "rust ownership borrowing memory safety");
        engine.add_document("mixed.txt", "rust and java interoperability");

        let results = engine.search("+rust -java", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "rust.txt");
    }
}
