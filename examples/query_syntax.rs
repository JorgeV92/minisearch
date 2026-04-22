use minisearch::{parse_query, SearchEngine};

fn main() {
    let parsed = parse_query("rust +\"distributed systems\"~2 -\"toy example\" -java");
    println!("{parsed:#?}");

    let mut engine = SearchEngine::new();
    engine.add_document("guide.txt", "rust distributed systems guide");
    engine.add_document("near.txt", "rust distributed storage systems guide");
    engine.add_document("toy.txt", "rust distributed systems toy example");

    for result in engine.search("rust +\"distributed systems\"~2 -\"toy example\"", 10) {
        println!("{} -> {:.3}", result.path, result.score);
    }
}
