use minisearch::{parse_query, SearchEngine};

fn main() {
    let parsed = parse_query("rust +\"phrase search\" -\"toy example\" -java");
    println!("{parsed:#?}");

    let mut engine = SearchEngine::new();
    engine.add_document("guide.txt", "rust phrase search guide");
    engine.add_document("toy.txt", "rust phrase search toy example");

    for result in engine.search("rust +\"phrase search\" -\"toy example\"", 10) {
        println!("{} -> {:.3}", result.path, result.score);
    }
}
