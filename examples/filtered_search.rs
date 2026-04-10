use minisearch::{SearchEngine, SearchOptions};

fn main() {
    let mut engine = SearchEngine::new();
    engine.add_document("guides/rust.md", "rust search engine rust phrase search");
    engine.add_document(
        "guides/storage.md",
        "storage engine persistence and indexing",
    );
    engine.add_document("notes/rust.md", "rust notes");

    let options = SearchOptions::new(5)
        .with_path_prefix("guides/")
        .with_min_score(1.0);

    for result in engine.search_with_options("rust", &options) {
        println!("{} -> {:.3}", result.path, result.score);
    }
}
