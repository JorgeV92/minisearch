# minisearch

`minisearch` is a small Rust search engine crate and CLI for indexing local text content with an inverted index, BM25-style scoring, phrase matching, and on-disk persistence.

## Features

- Recursive directory indexing for `.txt` and `.md` files by default
- Custom indexing options for file extensions and maximum file size
- Lowercased alphanumeric tokenization with positional postings
- BM25-style ranking for term queries
- Phrase search with quoted queries like `"distributed systems"`
- Required and excluded terms or phrases via `+term`, `-term`, `+"phrase"`, and `-"phrase"`
- Search-time filters for path prefixes and minimum score thresholds
- Simple save/load support for persisting an index to disk
- Lightweight stats helpers for vocabulary inspection and top terms

## Install

```bash
cargo add minisearch
```

To use the CLI locally:

```bash
cargo run -- <command>
```

## Quick Start

```rust
use minisearch::{SearchEngine, SearchOptions};

fn main() {
    let mut engine = SearchEngine::new();
    engine.add_document(
        "guides/project.txt",
        "A mini search engine in Rust with BM25 ranking and phrase search.",
    );
    engine.add_document(
        "notes/distributed.txt",
        "This document talks about distributed systems and indexing.",
    );

    let results = engine.search_with_options(
        "rust +\"phrase search\"",
        &SearchOptions::new(10).with_path_prefix("guides/"),
    );

    for result in results {
        println!(
            "{} -> {:.3} [{}]",
            result.path,
            result.score,
            result.matched_terms.join(", ")
        );
    }
}
```

## Query Syntax

| Syntax | Meaning | Example |
| --- | --- | --- |
| `rust bm25` | Optional terms ranked by BM25 | `rust bm25` |
| `+rust` | Required term | `+rust search` |
| `-java` | Excluded term | `rust -java` |
| `"phrase search"` | Phrase boost / phrase-only search | `"phrase search"` |
| `+"phrase search"` | Required phrase | `rust +"phrase search"` |
| `-"toy example"` | Excluded phrase | `rust -"toy example"` |

Notes:

- Optional terms contribute score when they appear.
- Required terms and required phrases must match for a document to be returned.
- Excluded terms and phrases remove a document from the result set.
- Phrase-only queries work even when no standalone terms are present.

## Library API

### Build an Index from a Directory

```rust
use minisearch::{IndexOptions, SearchEngine};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = IndexOptions::default()
        .with_extensions(["md", "txt", "rs"])
        .with_max_file_size_bytes(250_000);

    let engine = SearchEngine::build_from_directory_with_options("src", &options)?;
    println!("Indexed {} documents", engine.document_count());
    Ok(())
}
```

### Filter Search Results

```rust
use minisearch::{SearchEngine, SearchOptions};

fn main() {
    let mut engine = SearchEngine::new();
    engine.add_document("guides/rust.md", "rust search engine rust phrase search");
    engine.add_document("notes/rust.md", "rust notes");

    let options = SearchOptions::new(5)
        .with_path_prefix("guides/")
        .with_min_score(1.0);

    for result in engine.search_with_options("rust", &options) {
        println!("{} -> {:.3}", result.path, result.score);
    }
}
```

### Inspect the Vocabulary

```rust
use minisearch::SearchEngine;

fn main() {
    let mut engine = SearchEngine::new();
    engine.add_document("guide.txt", "rust rust search");
    engine.add_document("notes.txt", "rust indexing");

    println!("document frequency: {}", engine.document_frequency("rust"));
    println!("term frequency in doc 0: {}", engine.term_frequency(0, "rust"));

    for stat in engine.top_terms(3) {
        println!(
            "{} -> total {}, docs {}",
            stat.term, stat.total_frequency, stat.document_frequency
        );
    }
}
```

### Save and Reload an Index

```rust
use minisearch::SearchEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = SearchEngine::new();
    engine.add_document("guide.txt", "rust search engine rust bm25");
    engine.save_to_path("sample.idx")?;

    let loaded = SearchEngine::load_from_path("sample.idx")?;
    println!("loaded {} documents", loaded.document_count());
    Ok(())
}
```

## CLI

### Commands

```text
minisearch index <docs_dir> <index_file> [--ext=txt,md,rs] [--max-bytes=1048576]
minisearch search <index_file> <query> [top_k] [--path-prefix=guides/] [--min-score=1.0]
minisearch stats <index_file> [top_terms]
minisearch demo
```

### Examples

```bash
cargo run -- index docs search.idx --ext=txt,md,rs --max-bytes=100000
cargo run -- search search.idx 'rust +"phrase search"' 5 --path-prefix=guides/
cargo run -- search search.idx 'bm25' --min-score=1.0
cargo run -- stats search.idx 10
cargo run -- demo
```

## Included Examples

Run any example with `cargo run --example <name>`.

- `basic`: in-memory indexing plus filtered search
- `custom_indexing`: directory indexing with custom extensions and file size limits
- `filtered_search`: search-time path and score filters
- `persistence`: save/load and vocabulary statistics
- `query_syntax`: inspect parsed queries and required/excluded phrases

## Public Types

- `SearchEngine`: the main in-memory index
- `SearchOptions`: search-time filters like `top_k`, `path_prefix`, and `min_score`
- `IndexOptions`: directory indexing controls for extensions and max file size
- `SearchResult`: a matched document with score and matched terms
- `TermStat`: aggregated term statistics for reporting
- `ParsedQuery` / `PhraseQuery`: parsed query structures if you want to inspect or cache queries

## Persistence Format

Indexes are stored in a plain-text format that begins with the `MSE1` header and records:

- average document length
- document metadata
- positional postings for each term

The format is intentionally simple and human-readable, which makes it convenient for debugging and small tools.

## Development

```bash
cargo test
cargo test --examples
```
