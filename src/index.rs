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
    }
}