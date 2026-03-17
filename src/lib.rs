//! # Mini Search Engine
//!
//! A small Rust search engine 
//! The crate supports:
//!
//! - building an inverted index from text files,
//! - BM25-style ranking,
//! - exact phrase matching using positional postings,
//! - saving/loading an index from disk,
//! - and a small CLI wrapper for indexing and search.
//!

pub mod document;
pub mod index;
pub mod tokenizer;
pub mod query;
pub mod storage;

pub use document::DocumentMeta;