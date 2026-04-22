//! Core search engine implementation.

use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::document::DocumentMeta;
use crate::query::{
    parse_query, FuzzyTermQuery, MetadataField, MetadataFilter, ParsedQuery, PhraseQuery,
};
use crate::storage;
use crate::tokenizer::{tokenize, tokenize_with_positions, PositionedToken};

const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;
const DEFAULT_SNIPPET_MAX_CHARS: usize = 160;

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FuzzyMatch {
    term: String,
    distance: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedFuzzyTerm {
    query: FuzzyTermQuery,
    matches: Vec<FuzzyMatch>,
}

/// One search result returned by the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub doc_id: usize,
    pub path: String,
    pub score: f64,
    pub matched_terms: Vec<String>,
    pub snippet: Option<String>,
}

/// Aggregate statistics for one term in the vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermStat {
    pub term: String,
    pub document_frequency: usize,
    pub total_frequency: usize,
}

/// Directory indexing options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexOptions {
    extensions: Vec<String>,
    max_file_size_bytes: Option<u64>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            extensions: vec!["txt".to_string(), "md".to_string()],
            max_file_size_bytes: None,
        }
    }
}

impl IndexOptions {
    /// Creates the default indexing options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the supported file extensions.
    pub fn with_extensions<I, S>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut normalized = Vec::new();
        for extension in extensions {
            let extension = extension.as_ref().trim().trim_start_matches('.');
            if extension.is_empty() {
                continue;
            }

            let extension = extension.to_ascii_lowercase();
            if !normalized.iter().any(|existing| existing == &extension) {
                normalized.push(extension);
            }
        }
        self.extensions = normalized;
        self
    }

    /// Skips files larger than the provided byte size.
    pub fn with_max_file_size_bytes(mut self, max_file_size_bytes: u64) -> Self {
        self.max_file_size_bytes = Some(max_file_size_bytes);
        self
    }

    /// Returns the normalized extensions used while indexing.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Returns the configured maximum file size, if any.
    pub fn max_file_size_bytes(&self) -> Option<u64> {
        self.max_file_size_bytes
    }

    fn extension_set(&self) -> HashSet<String> {
        self.extensions.iter().cloned().collect()
    }
}

/// Search-time filtering options.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchOptions {
    pub top_k: usize,
    pub path_prefix: Option<String>,
    pub min_score: Option<f64>,
}

impl SearchOptions {
    /// Creates a new search configuration with the requested result limit.
    pub fn new(top_k: usize) -> Self {
        Self {
            top_k,
            path_prefix: None,
            min_score: None,
        }
    }

    /// Restricts results to documents whose stored path starts with the prefix.
    pub fn with_path_prefix(mut self, path_prefix: impl Into<String>) -> Self {
        self.path_prefix = Some(path_prefix.into());
        self
    }

    /// Drops matches whose score falls below the threshold.
    pub fn with_min_score(mut self, min_score: f64) -> Self {
        self.min_score = Some(min_score);
        self
    }
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

    /// Returns one document by its internal ID.
    pub fn document(&self, doc_id: usize) -> Option<&DocumentMeta> {
        self.documents.get(doc_id)
    }

    /// Returns the number of indexed documents.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Returns the number of unique terms in the lexicon.
    pub fn vocabulary_size(&self) -> usize {
        self.postings.len()
    }

    /// Returns the sorted vocabulary.
    pub fn vocabulary(&self) -> Vec<String> {
        let mut terms: Vec<_> = self.postings.keys().cloned().collect();
        terms.sort();
        terms
    }

    /// Returns the average document length in normalized tokens.
    pub fn average_document_length(&self) -> f64 {
        self.avg_doc_length
    }

    /// Returns how many documents contain the term.
    pub fn document_frequency(&self, term: &str) -> usize {
        let Some(term) = normalize_single_term(term) else {
            return 0;
        };
        self.postings
            .get(&term)
            .map(|postings| postings.len())
            .unwrap_or(0)
    }

    /// Returns the frequency of a term in one document.
    pub fn term_frequency(&self, doc_id: usize, term: &str) -> usize {
        let Some(term) = normalize_single_term(term) else {
            return 0;
        };
        self.positions_for_term_in_doc(&term, doc_id)
            .map(|positions| positions.len())
            .unwrap_or(0)
    }

    /// Reports whether the document contains the term.
    pub fn contains_term(&self, doc_id: usize, term: &str) -> bool {
        let Some(term) = normalize_single_term(term) else {
            return false;
        };
        self.contains_normalized_term(doc_id, &term)
    }

    /// Returns the most frequent terms in the vocabulary.
    pub fn top_terms(&self, limit: usize) -> Vec<TermStat> {
        if limit == 0 {
            return Vec::new();
        }

        let mut stats = self
            .postings
            .iter()
            .map(|(term, postings)| TermStat {
                term: term.clone(),
                document_frequency: postings.len(),
                total_frequency: postings.iter().map(Posting::term_frequency).sum(),
            })
            .collect::<Vec<_>>();

        stats.sort_by(|left, right| {
            right
                .total_frequency
                .cmp(&left.total_frequency)
                .then_with(|| right.document_frequency.cmp(&left.document_frequency))
                .then_with(|| left.term.cmp(&right.term))
        });
        stats.truncate(limit);
        stats
    }

    /// Adds one document to the engine.
    pub fn add_document(&mut self, path: impl Into<String>, content: &str) {
        self.add_document_with_metadata(path, content, None);
    }

    fn add_document_with_metadata(
        &mut self,
        path: impl Into<String>,
        content: &str,
        modified_unix_timestamp_secs: Option<u64>,
    ) {
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

        self.documents.push(DocumentMeta::with_metadata(
            doc_id,
            path,
            length,
            content,
            None,
            String::new(),
            modified_unix_timestamp_secs,
        ));
        self.recompute_average_length();
    }

    /// Builds an engine from a directory of `.txt` and `.md` files.
    pub fn build_from_directory(dir: impl AsRef<Path>) -> Result<Self, SearchError> {
        let mut engine = Self::new();
        engine.index_directory(dir)?;
        Ok(engine)
    }

    /// Builds an engine from a directory using custom indexing options.
    pub fn build_from_directory_with_options(
        dir: impl AsRef<Path>,
        options: &IndexOptions,
    ) -> Result<Self, SearchError> {
        let mut engine = Self::new();
        engine.index_directory_with_options(dir, options)?;
        Ok(engine)
    }

    /// Indexes all `.txt` and `.md` files under a directory recursively.
    pub fn index_directory(&mut self, dir: impl AsRef<Path>) -> Result<usize, SearchError> {
        self.index_directory_with_options(dir, &IndexOptions::default())
    }

    /// Indexes files under a directory recursively using custom options.
    pub fn index_directory_with_options(
        &mut self,
        dir: impl AsRef<Path>,
        options: &IndexOptions,
    ) -> Result<usize, SearchError> {
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
        let extension_set = options.extension_set();
        collect_supported_files(dir, &extension_set, &mut files)?;
        files.sort();

        let base = dir.to_path_buf();
        let start_count = self.document_count();

        for file in files {
            if let Some(max_file_size_bytes) = options.max_file_size_bytes() {
                let file_size = fs::metadata(&file)?.len();
                if file_size > max_file_size_bytes {
                    continue;
                }
            }

            let content = fs::read_to_string(&file)?;
            let modified_unix_timestamp_secs = fs::metadata(&file)?
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_secs());
            let relative = file
                .strip_prefix(&base)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| file.clone());
            self.add_document_with_metadata(
                relative.display().to_string(),
                &content,
                modified_unix_timestamp_secs,
            );
        }

        Ok(self.document_count() - start_count)
    }

    /// Searches the index using a raw query string.
    pub fn search(&self, raw_query: &str, top_k: usize) -> Vec<SearchResult> {
        self.search_with_options(raw_query, &SearchOptions::new(top_k))
    }

    /// Searches the index using a raw query string and extra search-time filters.
    pub fn search_with_options(
        &self,
        raw_query: &str,
        options: &SearchOptions,
    ) -> Vec<SearchResult> {
        let parsed = parse_query(raw_query);
        self.search_parsed_with_options(&parsed, options)
    }

    /// Searches the index using a pre-parsed query.
    pub fn search_parsed(&self, parsed: &ParsedQuery, top_k: usize) -> Vec<SearchResult> {
        self.search_parsed_with_options(parsed, &SearchOptions::new(top_k))
    }

    /// Searches the index using a pre-parsed query and extra search-time filters.
    pub fn search_parsed_with_options(
        &self,
        parsed: &ParsedQuery,
        options: &SearchOptions,
    ) -> Vec<SearchResult> {
        let top_k = options.top_k;
        if self.documents.is_empty() || top_k == 0 {
            return Vec::new();
        }

        let mut scores: HashMap<usize, f64> = HashMap::new();
        let mut matched_terms: HashMap<usize, HashSet<String>> = HashMap::new();
        let mut matched_token_terms: HashMap<usize, HashSet<String>> = HashMap::new();

        let resolved_optional_fuzzy = parsed
            .optional_fuzzy_terms
            .iter()
            .map(|query| self.resolve_fuzzy_term(query))
            .collect::<Vec<_>>();
        let resolved_required_fuzzy = parsed
            .required_fuzzy_terms
            .iter()
            .map(|query| self.resolve_fuzzy_term(query))
            .collect::<Vec<_>>();
        let resolved_excluded_fuzzy = parsed
            .excluded_fuzzy_terms
            .iter()
            .map(|query| self.resolve_fuzzy_term(query))
            .collect::<Vec<_>>();

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
                let score =
                    self.bm25_score(posting.doc_id, posting.term_frequency(), document_frequency);
                *scores.entry(posting.doc_id).or_insert(0.0) += score;
                matched_terms
                    .entry(posting.doc_id)
                    .or_default()
                    .insert(term.clone());
                matched_token_terms
                    .entry(posting.doc_id)
                    .or_default()
                    .insert(term.clone());
            }
        }

        for fuzzy_term in resolved_optional_fuzzy
            .iter()
            .chain(resolved_required_fuzzy.iter())
        {
            for candidate in &fuzzy_term.matches {
                let Some(postings) = self.postings.get(&candidate.term) else {
                    continue;
                };
                let document_frequency = postings.len();
                let score_multiplier = fuzzy_score_multiplier(candidate.distance);
                for posting in postings {
                    let score = self.bm25_score(
                        posting.doc_id,
                        posting.term_frequency(),
                        document_frequency,
                    ) * score_multiplier;
                    *scores.entry(posting.doc_id).or_insert(0.0) += score;
                    matched_terms
                        .entry(posting.doc_id)
                        .or_default()
                        .insert(candidate.term.clone());
                    matched_token_terms
                        .entry(posting.doc_id)
                        .or_default()
                        .insert(candidate.term.clone());
                }
            }
        }

        let has_scoring_terms = !parsed.optional_terms.is_empty()
            || !parsed.required_terms.is_empty()
            || !parsed.optional_fuzzy_terms.is_empty()
            || !parsed.required_fuzzy_terms.is_empty();
        let has_scoring_phrases = !parsed.phrases.is_empty() || !parsed.required_phrases.is_empty();
        let has_filter_only_clauses = !parsed.excluded_terms.is_empty()
            || !parsed.excluded_fuzzy_terms.is_empty()
            || !parsed.excluded_phrases.is_empty()
            || !parsed.required_metadata_filters.is_empty()
            || !parsed.excluded_metadata_filters.is_empty();
        let needs_full_doc_seed =
            !has_scoring_terms && (has_scoring_phrases || has_filter_only_clauses);
        if needs_full_doc_seed {
            for doc in &self.documents {
                scores.entry(doc.id).or_insert(0.0);
            }
        }

        for phrase in parsed.phrases.iter().chain(parsed.required_phrases.iter()) {
            for doc in &self.documents {
                if let Some(used_slop) = self.phrase_match_slop(doc.id, phrase) {
                    let boost = phrase_boost(phrase, used_slop);
                    *scores.entry(doc.id).or_insert(0.0) += boost;
                    matched_terms
                        .entry(doc.id)
                        .or_default()
                        .insert(phrase.terms.join(" "));
                }
            }
        }

        let mut results = Vec::new();
        'doc_loop: for (doc_id, score) in scores {
            if !self.satisfies_required_terms(doc_id, &parsed.required_terms) {
                continue;
            }
            if !self.satisfies_required_fuzzy_terms(doc_id, &resolved_required_fuzzy) {
                continue;
            }
            if self.matches_any_excluded_term(doc_id, &parsed.excluded_terms) {
                continue;
            }
            if self.matches_any_excluded_fuzzy_terms(doc_id, &resolved_excluded_fuzzy) {
                continue;
            }
            if !self.satisfies_metadata_filters(doc_id, &parsed.required_metadata_filters) {
                continue;
            }
            if self.matches_any_excluded_metadata_filters(doc_id, &parsed.excluded_metadata_filters)
            {
                continue;
            }
            if !self.satisfies_required_phrases(doc_id, &parsed.required_phrases) {
                continue;
            }
            if self.matches_any_excluded_phrases(doc_id, &parsed.excluded_phrases) {
                continue;
            }
            if !has_scoring_terms
                && has_scoring_phrases
                && !parsed
                    .phrases
                    .iter()
                    .chain(parsed.required_phrases.iter())
                    .any(|phrase| self.doc_has_phrase(doc_id, phrase))
            {
                continue;
            }

            let path = self.documents[doc_id].path.clone();
            if let Some(path_prefix) = options.path_prefix.as_deref() {
                if !path.starts_with(path_prefix) {
                    continue;
                }
            }
            if let Some(min_score) = options.min_score {
                if score < min_score {
                    continue;
                }
            }

            let highlight_terms = matched_token_terms.remove(&doc_id).unwrap_or_default();
            let mut terms = matched_terms
                .remove(&doc_id)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            terms.sort();

            for required in &parsed.required_terms {
                if !terms.iter().any(|term| term == required) {
                    if self.contains_normalized_term(doc_id, required) {
                        terms.push(required.clone());
                    } else {
                        continue 'doc_loop;
                    }
                }
            }
            for phrase in &parsed.required_phrases {
                let phrase_text = phrase.terms.join(" ");
                if !terms.iter().any(|term| term == &phrase_text) {
                    if self.doc_has_phrase(doc_id, phrase) {
                        terms.push(phrase_text);
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
                snippet: self.build_snippet(doc_id, parsed, &highlight_terms),
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
        required_terms
            .iter()
            .all(|term| self.contains_normalized_term(doc_id, term))
    }

    fn satisfies_required_fuzzy_terms(
        &self,
        doc_id: usize,
        required_terms: &[ResolvedFuzzyTerm],
    ) -> bool {
        required_terms.iter().all(|query| {
            query
                .matches
                .iter()
                .any(|candidate| self.contains_normalized_term(doc_id, &candidate.term))
        })
    }

    fn matches_any_excluded_term(&self, doc_id: usize, excluded_terms: &[String]) -> bool {
        excluded_terms
            .iter()
            .any(|term| self.contains_normalized_term(doc_id, term))
    }

    fn matches_any_excluded_fuzzy_terms(
        &self,
        doc_id: usize,
        excluded_terms: &[ResolvedFuzzyTerm],
    ) -> bool {
        excluded_terms.iter().any(|query| {
            query
                .matches
                .iter()
                .any(|candidate| self.contains_normalized_term(doc_id, &candidate.term))
        })
    }

    fn satisfies_metadata_filters(&self, doc_id: usize, filters: &[MetadataFilter]) -> bool {
        let Some(document) = self.documents.get(doc_id) else {
            return false;
        };
        filters
            .iter()
            .all(|filter| document_matches_metadata_filter(document, filter))
    }

    fn matches_any_excluded_metadata_filters(
        &self,
        doc_id: usize,
        filters: &[MetadataFilter],
    ) -> bool {
        let Some(document) = self.documents.get(doc_id) else {
            return false;
        };
        filters
            .iter()
            .any(|filter| document_matches_metadata_filter(document, filter))
    }

    fn satisfies_required_phrases(&self, doc_id: usize, required_phrases: &[PhraseQuery]) -> bool {
        required_phrases
            .iter()
            .all(|phrase| self.doc_has_phrase(doc_id, phrase))
    }

    fn matches_any_excluded_phrases(
        &self,
        doc_id: usize,
        excluded_phrases: &[PhraseQuery],
    ) -> bool {
        excluded_phrases
            .iter()
            .any(|phrase| self.doc_has_phrase(doc_id, phrase))
    }

    fn contains_normalized_term(&self, doc_id: usize, term: &str) -> bool {
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
        self.phrase_match_slop(doc_id, phrase).is_some()
    }

    fn phrase_match_slop(&self, doc_id: usize, phrase: &PhraseQuery) -> Option<usize> {
        if phrase.terms.is_empty() {
            return None;
        }
        if phrase.terms.len() == 1 {
            return self
                .contains_normalized_term(doc_id, &phrase.terms[0])
                .then_some(0);
        }

        let mut position_lists: Vec<&[usize]> = Vec::with_capacity(phrase.terms.len());
        for term in &phrase.terms {
            let Some(positions) = self.positions_for_term_in_doc(term, doc_id) else {
                return None;
            };
            position_lists.push(positions);
        }

        let mut best_match: Option<usize> = None;
        for &start in position_lists[0] {
            if let Some(used_slop) =
                find_phrase_match_with_slop(&position_lists, 1, start, 0, phrase.slop)
            {
                best_match = Some(best_match.map_or(used_slop, |best| best.min(used_slop)));
                if used_slop == 0 {
                    break;
                }
            }
        }

        best_match
    }

    fn build_snippet(
        &self,
        doc_id: usize,
        parsed: &ParsedQuery,
        matched_token_terms: &HashSet<String>,
    ) -> Option<String> {
        let content = &self.documents.get(doc_id)?.content;
        if content.is_empty() {
            return None;
        }

        let tokens = tokenize_with_positions(content);
        if tokens.is_empty() {
            return None;
        }

        let mut ranges = collect_match_ranges(&tokens, parsed, matched_token_terms);
        if ranges.is_empty() {
            return None;
        }

        merge_ranges(&mut ranges);
        let snippet_range = choose_snippet_range(content, ranges[0], DEFAULT_SNIPPET_MAX_CHARS);
        let visible_ranges = clip_ranges(&ranges, snippet_range);
        Some(render_highlighted_snippet(
            content,
            snippet_range,
            &visible_ranges,
        ))
    }

    fn resolve_fuzzy_term(&self, query: &FuzzyTermQuery) -> ResolvedFuzzyTerm {
        let mut matches = self
            .postings
            .keys()
            .filter_map(|term| {
                bounded_levenshtein(&query.term, term, query.max_distance).map(|distance| {
                    FuzzyMatch {
                        term: term.clone(),
                        distance,
                    }
                })
            })
            .collect::<Vec<_>>();

        matches.sort_by(|left, right| {
            left.distance
                .cmp(&right.distance)
                .then_with(|| left.term.cmp(&right.term))
        });

        ResolvedFuzzyTerm {
            query: query.clone(),
            matches,
        }
    }
}

fn normalize_single_term(term: &str) -> Option<String> {
    let mut tokens = tokenize(term);
    if tokens.len() == 1 {
        tokens.pop()
    } else {
        None
    }
}

fn document_matches_metadata_filter(document: &DocumentMeta, filter: &MetadataFilter) -> bool {
    match filter.field {
        MetadataField::Extension => document.extension.as_deref() == Some(filter.value.as_str()),
        MetadataField::Path => document.path.starts_with(&filter.value),
        MetadataField::Title => {
            let title_terms = tokenize(&document.title);
            let filter_terms = tokenize(&filter.value);
            if filter_terms.is_empty() {
                return false;
            }
            filter_terms
                .iter()
                .all(|term| title_terms.iter().any(|candidate| candidate == term))
        }
    }
}

fn collect_supported_files(
    dir: &Path,
    extensions: &HashSet<String>,
    files: &mut Vec<PathBuf>,
) -> Result<(), SearchError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_supported_files(&path, extensions, files)?;
        } else if is_supported_file(&path, extensions) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_supported_file(path: &Path, extensions: &HashSet<String>) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    extensions.contains(&extension.to_ascii_lowercase())
}

fn collect_match_ranges(
    tokens: &[PositionedToken],
    parsed: &ParsedQuery,
    matched_token_terms: &HashSet<String>,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();

    for term in matched_token_terms {
        for token in tokens.iter().filter(|token| token.term == *term) {
            ranges.push((token.start, token.end));
        }
    }

    for phrase in parsed.phrases.iter().chain(parsed.required_phrases.iter()) {
        ranges.extend(collect_phrase_ranges(tokens, phrase));
    }

    ranges
}

fn collect_phrase_ranges(tokens: &[PositionedToken], phrase: &PhraseQuery) -> Vec<(usize, usize)> {
    if phrase.terms.is_empty() {
        return Vec::new();
    }

    if phrase.terms.len() == 1 {
        return tokens
            .iter()
            .filter(|token| token.term == phrase.terms[0])
            .map(|token| (token.start, token.end))
            .collect();
    }

    let mut position_lists = Vec::with_capacity(phrase.terms.len());
    for term in &phrase.terms {
        let positions = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| (token.term == *term).then_some(index))
            .collect::<Vec<_>>();
        if positions.is_empty() {
            return Vec::new();
        }
        position_lists.push(positions);
    }

    let mut matches = Vec::new();
    for &start_index in &position_lists[0] {
        collect_phrase_match_ranges(
            &position_lists,
            1,
            start_index,
            0,
            phrase.slop,
            start_index,
            &mut matches,
        );
    }

    matches
        .into_iter()
        .map(|(start_index, end_index)| (tokens[start_index].start, tokens[end_index].end))
        .collect()
}

fn collect_phrase_match_ranges(
    position_lists: &[Vec<usize>],
    term_index: usize,
    previous_position: usize,
    used_slop: usize,
    max_slop: usize,
    start_position: usize,
    matches: &mut Vec<(usize, usize)>,
) {
    if term_index == position_lists.len() {
        matches.push((start_position, previous_position));
        return;
    }

    for &position in &position_lists[term_index] {
        if position <= previous_position {
            continue;
        }

        let next_slop = used_slop + (position - previous_position - 1);
        if next_slop > max_slop {
            break;
        }

        collect_phrase_match_ranges(
            position_lists,
            term_index + 1,
            position,
            next_slop,
            max_slop,
            start_position,
            matches,
        );
    }
}

fn merge_ranges(ranges: &mut Vec<(usize, usize)>) {
    ranges.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let mut merged = Vec::with_capacity(ranges.len());
    for &(start, end) in ranges.iter() {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= *last_end {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    *ranges = merged;
}

fn clip_ranges(ranges: &[(usize, usize)], snippet_range: (usize, usize)) -> Vec<(usize, usize)> {
    let (snippet_start, snippet_end) = snippet_range;
    ranges
        .iter()
        .filter_map(|&(start, end)| {
            let clipped_start = start.max(snippet_start);
            let clipped_end = end.min(snippet_end);
            (clipped_start < clipped_end).then_some((clipped_start, clipped_end))
        })
        .collect()
}

fn choose_snippet_range(
    content: &str,
    anchor_range: (usize, usize),
    max_chars: usize,
) -> (usize, usize) {
    let target = max_chars.max(anchor_range.1.saturating_sub(anchor_range.0));
    let extra = target.saturating_sub(anchor_range.1.saturating_sub(anchor_range.0));
    let left_context = extra / 2;
    let right_context = extra - left_context;

    let mut start = anchor_range.0.saturating_sub(left_context);
    let mut end = (anchor_range.1 + right_context).min(content.len());

    let current_len = end.saturating_sub(start);
    if current_len < target {
        let missing = target - current_len;
        let extend_left = missing.min(start);
        start -= extend_left;
        end = (end + (missing - extend_left)).min(content.len());
    }

    start = clamp_to_char_boundary_left(content, start);
    end = clamp_to_char_boundary_right(content, end);
    start = trim_leading_whitespace(content, start, end);
    end = trim_trailing_whitespace(content, start, end);

    (start, end)
}

fn render_highlighted_snippet(
    content: &str,
    snippet_range: (usize, usize),
    highlight_ranges: &[(usize, usize)],
) -> String {
    let (snippet_start, snippet_end) = snippet_range;
    let mut snippet = String::new();
    if snippet_start > 0 {
        snippet.push_str("...");
    }

    let mut range_index = 0usize;
    let mut in_highlight = false;
    let mut last_was_space = false;

    for (offset, ch) in content[snippet_start..snippet_end].char_indices() {
        let global_start = snippet_start + offset;
        let global_end = global_start + ch.len_utf8();

        if range_index < highlight_ranges.len() && highlight_ranges[range_index].0 == global_start {
            snippet.push_str("[[");
            in_highlight = true;
        }

        if ch.is_whitespace() {
            if !last_was_space {
                snippet.push(' ');
                last_was_space = true;
            }
        } else {
            snippet.push(ch);
            last_was_space = false;
        }

        if in_highlight
            && range_index < highlight_ranges.len()
            && highlight_ranges[range_index].1 == global_end
        {
            snippet.push_str("]]");
            in_highlight = false;
            range_index += 1;
        }
    }

    if in_highlight {
        snippet.push_str("]]");
    }
    if snippet_end < content.len() {
        snippet.push_str("...");
    }

    snippet.trim().to_string()
}

fn clamp_to_char_boundary_left(content: &str, mut index: usize) -> usize {
    index = index.min(content.len());
    while index > 0 && !content.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn clamp_to_char_boundary_right(content: &str, mut index: usize) -> usize {
    index = index.min(content.len());
    while index < content.len() && !content.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn trim_leading_whitespace(content: &str, mut start: usize, end: usize) -> usize {
    while start < end {
        let ch = content[start..end].chars().next().unwrap();
        if !ch.is_whitespace() {
            break;
        }
        start += ch.len_utf8();
    }
    start
}

fn trim_trailing_whitespace(content: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let ch = content[start..end].chars().next_back().unwrap();
        if !ch.is_whitespace() {
            break;
        }
        end -= ch.len_utf8();
    }
    end
}

fn fuzzy_score_multiplier(distance: usize) -> f64 {
    1.0 / (1.0 + distance as f64)
}

fn bounded_levenshtein(left: &str, right: &str, max_distance: usize) -> Option<usize> {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();

    if left_chars.len().abs_diff(right_chars.len()) > max_distance {
        return None;
    }

    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left_chars.iter().enumerate() {
        current[0] = left_index + 1;
        let mut row_min = current[0];

        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution_cost);
            row_min = row_min.min(current[right_index + 1]);
        }

        if row_min > max_distance {
            return None;
        }

        std::mem::swap(&mut previous, &mut current);
    }

    let distance = previous[right_chars.len()];
    (distance <= max_distance).then_some(distance)
}

fn find_phrase_match_with_slop(
    position_lists: &[&[usize]],
    term_index: usize,
    previous_position: usize,
    used_slop: usize,
    max_slop: usize,
) -> Option<usize> {
    if term_index == position_lists.len() {
        return Some(used_slop);
    }

    let mut best_match: Option<usize> = None;
    for &position in position_lists[term_index] {
        if position <= previous_position {
            continue;
        }

        let next_slop = used_slop + (position - previous_position - 1);
        if next_slop > max_slop {
            break;
        }

        if let Some(match_slop) = find_phrase_match_with_slop(
            position_lists,
            term_index + 1,
            position,
            next_slop,
            max_slop,
        ) {
            best_match = Some(best_match.map_or(match_slop, |best| best.min(match_slop)));
            if match_slop == used_slop {
                break;
            }
        }
    }

    best_match
}

fn phrase_boost(phrase: &PhraseQuery, used_slop: usize) -> f64 {
    (2.0 * phrase.terms.len() as f64) / (1.0 + used_slop as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn proximity_phrase_matches_within_slop() {
        let mut engine = SearchEngine::new();
        engine.add_document("exact.txt", "distributed systems are fun");
        engine.add_document("near.txt", "distributed storage and systems are fun");
        engine.add_document("reversed.txt", "systems and distributed are mentioned");

        let exact_results = engine.search("\"distributed systems\"", 10);
        assert_eq!(exact_results.len(), 1);
        assert_eq!(exact_results[0].path, "exact.txt");

        let proximity_results = engine.search("\"distributed systems\"~2", 10);
        assert_eq!(proximity_results.len(), 2);
        assert_eq!(proximity_results[0].path, "exact.txt");
        assert_eq!(proximity_results[1].path, "near.txt");
    }

    #[test]
    fn fuzzy_search_matches_typos_and_prefers_exact_hits() {
        let mut engine = SearchEngine::new();
        engine.add_document("exact.txt", "search engine");
        engine.add_document("fuzzy.txt", "serch engine");

        let results = engine.search("serch~1", 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, "fuzzy.txt");
        assert_eq!(results[1].path, "exact.txt");
        assert!(results[1].matched_terms.iter().any(|term| term == "search"));
    }

    #[test]
    fn required_and_excluded_fuzzy_terms_filter_results() {
        let mut engine = SearchEngine::new();
        engine.add_document("guide.txt", "rust tokenizer guide");
        engine.add_document("mixed.txt", "rust tokeniser guide java");
        engine.add_document("other.txt", "rust parsing guide");

        let results = engine.search("+tokenzier~2 -jave~1 rust", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "guide.txt");
    }

    #[test]
    fn metadata_filters_match_extension_path_and_title() {
        let mut engine = SearchEngine::new();
        engine.add_document("guides/rust_intro.md", "# Rust Search\nbody");
        engine.add_document("guides/generated.rs", "pub fn generated() {}");
        engine.add_document("notes/rust.txt", "rust notes");

        let extension_results = engine.search("ext:md", 10);
        assert_eq!(extension_results.len(), 1);
        assert_eq!(extension_results[0].path, "guides/rust_intro.md");

        let path_results = engine.search("path:guides/", 10);
        assert_eq!(path_results.len(), 2);

        let title_results = engine.search("title:search", 10);
        assert_eq!(title_results.len(), 1);
        assert_eq!(title_results[0].path, "guides/rust_intro.md");
    }

    #[test]
    fn metadata_filters_can_exclude_results() {
        let mut engine = SearchEngine::new();
        engine.add_document("guides/search.md", "# Search Guide\nbody");
        engine.add_document("guides/generated.rs", "pub fn generated() {}");

        let results = engine.search("path:guides/ -ext:rs -title:generated", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "guides/search.md");
    }

    #[test]
    fn search_results_include_highlighted_snippets() {
        let mut engine = SearchEngine::new();
        engine.add_document(
            "guide.txt",
            "Rust search-engine results can show snippets and highlighting.",
        );

        let results = engine.search("\"search engine\" rust", 10);
        let snippet = results[0].snippet.as_deref().unwrap();
        assert!(snippet.contains("[[Rust]]"));
        assert!(snippet.contains("[[search-engine]]"));
    }

    #[test]
    fn fuzzy_matches_are_highlighted_in_snippets() {
        let mut engine = SearchEngine::new();
        engine.add_document(
            "guide.txt",
            "The tokenizer supports search suggestions and typo correction.",
        );

        let results = engine.search("serch~1", 10);
        let snippet = results[0].snippet.as_deref().unwrap();
        assert!(snippet.contains("[[search]]"));
    }

    #[test]
    fn required_proximity_phrase_filters_results() {
        let mut engine = SearchEngine::new();
        engine.add_document("guide.txt", "rust distributed systems guide");
        engine.add_document("near.txt", "rust distributed storage systems guide");
        engine.add_document(
            "far.txt",
            "rust distributed storage indexing examples systems guide",
        );

        let results = engine.search("+\"distributed systems\"~2 rust", 10);
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|result| result.path == "guide.txt"));
        assert!(results.iter().any(|result| result.path == "near.txt"));
        assert!(!results.iter().any(|result| result.path == "far.txt"));
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

    #[test]
    fn required_and_excluded_phrases_filter_results() {
        let mut engine = SearchEngine::new();
        engine.add_document(
            "guide.txt",
            "a search engine with phrase search and bm25 ranking",
        );
        engine.add_document("toy.txt", "a search engine toy example with phrase search");

        let results = engine.search("+\"search engine\" -\"toy example\"", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "guide.txt");
    }

    #[test]
    fn search_options_filter_results() {
        let mut engine = SearchEngine::new();
        engine.add_document("guides/rust.txt", "rust search engine rust rust");
        engine.add_document("notes/rust.txt", "rust search engine");

        let results = engine.search_with_options(
            "rust",
            &SearchOptions::new(10)
                .with_path_prefix("guides/")
                .with_min_score(0.1),
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "guides/rust.txt");
    }

    #[test]
    fn index_options_control_extensions_and_file_size() {
        let dir = unique_temp_path("index_options");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("guide.md"), "search engine docs").unwrap();
        fs::write(dir.join("lib.rs"), "rust search engine").unwrap();
        fs::write(dir.join("large.txt"), "this file is definitely too large").unwrap();

        let options = IndexOptions::default()
            .with_extensions(["md", "rs", "txt"])
            .with_max_file_size_bytes(20);
        let engine = SearchEngine::build_from_directory_with_options(&dir, &options).unwrap();

        assert_eq!(engine.document_count(), 2);
        assert!(engine.documents().iter().any(|doc| doc.path == "guide.md"));
        assert!(engine.documents().iter().any(|doc| doc.path == "lib.rs"));
        assert!(!engine.documents().iter().any(|doc| doc.path == "large.txt"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn term_statistics_are_exposed() {
        let mut engine = SearchEngine::new();
        engine.add_document("a.txt", "rust rust search");
        engine.add_document("b.txt", "rust indexing");

        assert_eq!(engine.document_frequency("Rust"), 2);
        assert_eq!(engine.term_frequency(0, "rust"), 2);
        assert!(engine.contains_term(1, "rust"));
        assert_eq!(engine.top_terms(1)[0].term, "rust");
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("minisearch_{name}_{nanos}"))
    }
}
