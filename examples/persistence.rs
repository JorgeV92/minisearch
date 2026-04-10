use std::fs;

use minisearch::SearchEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let index_path = std::env::temp_dir().join("minisearch_persistence_example.idx");

    let mut engine = SearchEngine::new();
    engine.add_document("guide.txt", "rust search engine rust bm25");
    engine.add_document("notes.txt", "phrase search and persistence");
    engine.save_to_path(&index_path)?;

    let loaded = SearchEngine::load_from_path(&index_path)?;
    for stat in loaded.top_terms(3) {
        println!(
            "{} -> total {}, docs {}",
            stat.term, stat.total_frequency, stat.document_frequency
        );
    }

    let _ = fs::remove_file(index_path);
    Ok(())
}
