use std::env;
use std::process;

use minisearch::{IndexOptions, SearchEngine, SearchError, SearchOptions};

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
        "index" => run_index(&args[0], &args[2..])?,
        "search" => run_search(&args[0], &args[2..])?,
        "stats" => run_stats(&args[0], &args[2..])?,
        "demo" => run_demo(),
        _ => print_usage(&args[0]),
    }

    Ok(())
}

fn run_index(bin_name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        print_usage(bin_name);
        return Ok(());
    }

    let docs_dir = &args[0];
    let index_path = &args[1];
    let options = parse_index_options(&args[2..])?;

    let engine = SearchEngine::build_from_directory_with_options(docs_dir, &options)?;
    engine.save_to_path(index_path)?;

    println!("Indexed {} documents", engine.document_count());
    println!("Vocabulary size: {}", engine.vocabulary_size());
    println!(
        "Average document length: {:.2}",
        engine.average_document_length()
    );
    println!("Extensions: {}", format_extensions(options.extensions()));
    if let Some(max_file_size_bytes) = options.max_file_size_bytes() {
        println!("Max file size: {max_file_size_bytes} bytes");
    }
    println!("Saved index to {index_path}");

    Ok(())
}

fn run_search(bin_name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        print_usage(bin_name);
        return Ok(());
    }

    let index_path = &args[0];
    let query = &args[1];
    let (top_k, path_prefix, min_score) = parse_search_args(&args[2..])?;

    let options = {
        let mut options = SearchOptions::new(top_k);
        if let Some(path_prefix) = path_prefix {
            options = options.with_path_prefix(path_prefix);
        }
        if let Some(min_score) = min_score {
            options = options.with_min_score(min_score);
        }
        options
    };

    let engine = SearchEngine::load_from_path(index_path)?;
    let results = engine.search_with_options(query, &options);
    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    for (rank, result) in results.iter().enumerate() {
        println!("{}. {} (score: {:.3})", rank + 1, result.path, result.score);
        if !result.matched_terms.is_empty() {
            println!("   matched: {}", result.matched_terms.join(", "));
        }
        if let Some(snippet) = &result.snippet {
            println!("   snippet: {snippet}");
        }
    }

    Ok(())
}

fn run_stats(bin_name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() || args.len() > 2 {
        print_usage(bin_name);
        return Ok(());
    }

    let index_path = &args[0];
    let top_terms = if args.len() == 2 {
        args[1].parse::<usize>()?
    } else {
        10
    };

    let engine = SearchEngine::load_from_path(index_path)?;
    println!("Documents: {}", engine.document_count());
    println!("Vocabulary size: {}", engine.vocabulary_size());
    println!(
        "Average document length: {:.2}",
        engine.average_document_length()
    );

    let top_terms = engine.top_terms(top_terms);
    if top_terms.is_empty() {
        println!("Top terms: none");
        return Ok(());
    }

    println!("Top terms:");
    for stat in top_terms {
        println!(
            "  {} -> total {}, docs {}",
            stat.term, stat.total_frequency, stat.document_frequency
        );
    }

    Ok(())
}

fn run_demo() {
    let mut engine = SearchEngine::new();
    engine.add_document(
        "guides/search.txt",
        "Rust project with inverted index bm25 ranking and phrase search",
    );
    engine.add_document(
        "notes/distributed.txt",
        "Distributed systems and storage engines are great interview topics",
    );
    engine.add_document(
        "guides/query.txt",
        "Quoted phrase search can require exact matches and skip toy examples",
    );

    let results = engine.search_with_options(
        "+\"phrase search\" -\"toy examples\" rust",
        &SearchOptions::new(3).with_path_prefix("guides/"),
    );

    for (rank, result) in results.iter().enumerate() {
        println!(
            "{}. {} (score: {:.3}) [{}]",
            rank + 1,
            result.path,
            result.score,
            result.matched_terms.join(", ")
        );
        if let Some(snippet) = &result.snippet {
            println!("   {snippet}");
        }
    }
}

fn parse_index_options(args: &[String]) -> Result<IndexOptions, SearchError> {
    let mut options = IndexOptions::default();

    for arg in args {
        if let Some(extensions) = arg.strip_prefix("--ext=") {
            options = options.with_extensions(extensions.split(','));
        } else if let Some(value) = arg.strip_prefix("--max-bytes=") {
            let max_file_size_bytes = value.parse::<u64>().map_err(|_| {
                SearchError::InvalidArgument(format!("invalid value for --max-bytes: {value}"))
            })?;
            options = options.with_max_file_size_bytes(max_file_size_bytes);
        } else {
            return Err(SearchError::InvalidArgument(format!(
                "unknown index option: {arg}"
            )));
        }
    }

    Ok(options)
}

fn parse_search_args(args: &[String]) -> Result<(usize, Option<String>, Option<f64>), SearchError> {
    let mut top_k = 5;
    let mut seen_top_k = false;
    let mut path_prefix = None;
    let mut min_score = None;

    for arg in args {
        if let Some(value) = arg.strip_prefix("--path-prefix=") {
            path_prefix = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--min-score=") {
            min_score = Some(value.parse::<f64>().map_err(|_| {
                SearchError::InvalidArgument(format!("invalid value for --min-score: {value}"))
            })?);
        } else if !seen_top_k {
            top_k = arg.parse::<usize>().map_err(|_| {
                SearchError::InvalidArgument(format!("expected top_k or search option, got: {arg}"))
            })?;
            seen_top_k = true;
        } else {
            return Err(SearchError::InvalidArgument(format!(
                "unexpected search argument: {arg}"
            )));
        }
    }

    Ok((top_k, path_prefix, min_score))
}

fn format_extensions(extensions: &[String]) -> String {
    if extensions.is_empty() {
        "none".to_string()
    } else {
        extensions.join(", ")
    }
}

fn print_usage(bin_name: &str) {
    println!("minisearch");
    println!();
    println!("Usage:");
    println!("  {bin_name} index <docs_dir> <index_file> [--ext=txt,md,rs] [--max-bytes=1048576]");
    println!(
        "  {bin_name} search <index_file> <query> [top_k] [--path-prefix=guides/] [--min-score=1.0]"
    );
    println!("  {bin_name} stats <index_file> [top_terms]");
    println!("  {bin_name} demo");
    println!();
    println!("Examples:");
    println!("  {bin_name} index sample_docs sample.idx --ext=txt,md,rs");
    println!("  {bin_name} index sample_docs sample.idx --max-bytes=50000");
    println!("  {bin_name} search sample.idx 'rust +\"phrase search\"' 5 --path-prefix=guides/");
    println!("  {bin_name} search sample.idx 'bm25' --min-score=1.0");
    println!("  {bin_name} stats sample.idx 10");
}
