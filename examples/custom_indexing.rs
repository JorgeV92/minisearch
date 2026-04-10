use std::fs;

use minisearch::{IndexOptions, SearchEngine};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = std::env::temp_dir().join("minisearch_custom_indexing_example");
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("README.md"), "Mini search engine guide")?;
    fs::write(workspace.join("lib.rs"), "pub fn search() {}")?;
    fs::write(
        workspace.join("large.txt"),
        "this file will be skipped by the size limit",
    )?;

    let options = IndexOptions::default()
        .with_extensions(["md", "rs", "txt"])
        .with_max_file_size_bytes(24);
    let engine = SearchEngine::build_from_directory_with_options(&workspace, &options)?;

    println!("Indexed {} documents", engine.document_count());
    for doc in engine.documents() {
        println!("{} ({} tokens)", doc.path, doc.length);
    }

    let _ = fs::remove_dir_all(workspace);
    Ok(())
}
