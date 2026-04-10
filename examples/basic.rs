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
        "rust \"phrase search\"",
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
