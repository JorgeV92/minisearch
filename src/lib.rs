//! # minisearch
//!
//! A small Rust search engine with:
//!
//! - recursive indexing for local files,
//! - BM25-style ranking,
//! - quoted phrase and proximity matching with positional postings,
//! - fuzzy term matching for typo tolerance,
//! - metadata filters like `ext:rs` and `path:src/`,
//! - highlighted result snippets from stored document text,
//! - configurable indexing and search filters,
//! - and a simple persistence format for saving/loading indexes.
//!
//! ```rust
//! use minisearch::{SearchEngine, SearchOptions};
//!
//! let mut engine = SearchEngine::new();
//! engine.add_document(
//!     "guides/rust.txt",
//!     "A mini search engine in Rust with phrase search and BM25 ranking.",
//! );
//! engine.add_document(
//!     "notes/distributed.txt",
//!     "Distributed systems notes with indexing examples.",
//! );
//!
//! let results = engine.search_with_options(
//!     "rust \"phrase search\"",
//!     &SearchOptions::new(5).with_path_prefix("guides/"),
//! );
//!
//! assert_eq!(results.len(), 1);
//! assert_eq!(results[0].path, "guides/rust.txt");
//! ```

pub mod document;
#[cfg(feature = "python-bindings")]
mod ffi;
pub mod index;
pub mod query;
pub mod storage;
pub mod tokenizer;

pub use document::DocumentMeta;
pub use index::{IndexOptions, SearchEngine, SearchError, SearchOptions, SearchResult, TermStat};
pub use query::{
    parse_query, FuzzyTermQuery, MetadataField, MetadataFilter, ParsedQuery, PhraseQuery,
};
