import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from minisearch import SearchEngine, SearchOptions


def main() -> None:
    engine = SearchEngine()
    engine.add_document(
        "guides/rust.txt",
        "A mini search engine in Rust with phrase search and BM25 ranking.",
    )
    engine.add_document(
        "notes/python.txt",
        "Python can call the Rust backend through the minisearch bindings.",
    )

    results = engine.search("rust", SearchOptions(top_k=5))
    for result in results:
        print(f"{result.path} -> {result.score:.3f}")
        if result.snippet:
            print(f"snippet: {result.snippet}")


if __name__ == "__main__":
    main()
