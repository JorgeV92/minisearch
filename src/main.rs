use std::env;
use std::process;

use::minisearch::SearchEngine;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage(&args[0]);
        return Ok(());
    }

    match args[1].as_str() {
        "index" => {
            if args.len() != 4 {
                print_usage(&args[0]);
                return Ok(());
            }
            let docs_dir = &args[2];
            let index_path = &args[3];
            let engine = SearchEngine::build_from_directory(docs_dir)?;
            engine.save_to_path(index_path)?;
            println!("Indexed {} documents", engine.document_count());
            println!("Vocabulary size: {}", engine.vocabulary_size());
            println!("Saved index to {index_path}");
        }
        "search" => {
            if args.len() < 4 || args.len() > 5 {
                print_usage(&args[0]);
                return Ok(());
            }
            let index_path = &args[2];
            let query = &args[3];
            let top_k = if args.len() == 5 {
                args[4].parse::<usize>()?
            } else {
                5
            };

            let engine = SearchEngine::load_from_path(index_path)?;
            let results = engine.search(query, top_k);
            if results.is_empty() {
                println!("No results found.");
                return Ok(());
            }

            for (rank, result) in results.iter().enumerate() {
                println!(
                    "{}. {} (score: {:.3})",
                    rank + 1,
                    result.path,
                    result.score
                );
                if !result.matched_terms.is_empty() {
                    println!("   matched: {}", result.matched_terms.join(", "));
                }
            }
        }
        "demo" => {
            let mut engine = SearchEngine::new();
            engine.add_document(
                "resume_notes.txt",
                "Rust project with inverted index bm25 ranking and phrase search",
            );
            engine.add_document(
                "distributed_systems.txt",
                "Distributed systems and storage engines are great interview topics",
            );
            engine.add_document(
                "compiler_notes.txt",
                "Compilers and static analysis are fascinating systems subjects",
            );

            let results = engine.search("rust \"phrase search\"", 3);
            for (rank, result) in results.iter().enumerate() {
                println!(
                    "{}. {} (score: {:.3}) [{}]",
                    rank + 1,
                    result.path,
                    result.score,
                    result.matched_terms.join(", ")
                );
            }
        }
        _ => {
            print_usage(&args[0]);
        }
    }

    Ok(())
}

fn print_usage(bin_name: &str) {
    println!("Mini Search Engine");
    println!();
    println!("Usage:");
    println!("  {bin_name} index <docs_dir> <index_file>");
    println!("  {bin_name} search <index_file> <query> [top_k]");
    println!("  {bin_name} demo");
    println!();
    println!("Examples:");
    println!("  {bin_name} index sample_docs sample.idx");
    println!("  {bin_name} search sample.idx 'rust bm25' 3");
    println!("  {bin_name} search sample.idx '\"distributed systems\"' 5");
}
