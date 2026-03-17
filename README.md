# MiniSearch

### Search with rust 

This project builds an **inverted index** over local text files, supports **BM25-style ranking**, **quoted phrase search**, and **saving/loading an index from disk**.

## Features

- Indexes `.txt` and `.md` files recursively from a directory
- Normalizes text to lowercase alphanumeric tokens
- Stores **term positions** for exact phrase matching
- Supports query syntax with:
  - plain terms: `rust bm25`
  - required terms: `+rust`
  - excluded terms: `-java`
  - quoted phrases: `"distributed systems"`

  ## Example library usage

```rust
use mini_search_engine::SearchEngine;

fn main() {
    let mut engine = SearchEngine::new();
    engine.add_document(
        "project.txt",
        "A mini search engine in Rust with BM25 ranking and phrase search.",
    );
    engine.add_document(
        "notes.txt",
        "This document talks about distributed systems and indexing.",
    );

    let results = engine.search("rust \"phrase search\"", 10);
    for result in results {
        println!("{} -> {:.3}", result.path, result.score);
    }
}
```