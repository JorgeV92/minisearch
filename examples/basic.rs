use minisearch::SearchEngine;

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
