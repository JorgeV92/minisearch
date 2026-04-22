use minisearch::{parse_query, SearchEngine};

fn main() {
    let parsed = parse_query("path:guides/ ext:md title:search serch~1 +\"distributed systems\"~2");
    println!("{parsed:#?}");

    let mut engine = SearchEngine::new();
    engine.add_document(
        "guides/search.md",
        "# Search Guide\nrust distributed systems search guide",
    );
    engine.add_document(
        "guides/near.md",
        "# Nearby Search\nrust distributed storage systems search guide",
    );
    engine.add_document("notes/search.txt", "rust distributed systems search guide");

    for result in engine.search(
        "path:guides/ ext:md title:search serch~1 +\"distributed systems\"~2",
        10,
    ) {
        println!(
            "{} -> {:.3} [{}]",
            result.path,
            result.score,
            result.matched_terms.join(", ")
        );
        if let Some(snippet) = result.snippet {
            println!("   {snippet}");
        }
    }
}
