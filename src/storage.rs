//! Index persistence.
//!
//! The on-disk format is intentionally simple and human-readable:
//!
//! ```text
//! MSE3
//! AVG\t3.500000
//! DOC\t0\tREADME.md\t120\tDocument text content\tmd\tREADME\t1712793600
//! POST\trust\t0\t0,9,42
//! ```
//!

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::document::DocumentMeta;
use crate::index::{Posting, SearchEngine, SearchError};

const MAGIC_HEADER_V1: &str = "MSE1";
const MAGIC_HEADER_V2: &str = "MSE2";
const MAGIC_HEADER_V3: &str = "MSE3";

pub(crate) fn save_engine(engine: &SearchEngine, path: &Path) -> Result<(), SearchError> {
    let mut lines = Vec::new();
    lines.push(MAGIC_HEADER_V3.to_string());
    lines.push(format!("AVG\t{:.12}", engine.average_document_length()));

    for doc in engine.documents() {
        lines.push(format!(
            "DOC\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            doc.id,
            escape_field(&doc.path),
            doc.length,
            escape_field(&doc.content),
            escape_field(doc.extension.as_deref().unwrap_or("")),
            escape_field(&doc.title),
            doc.modified_unix_timestamp_secs
                .map(|value| value.to_string())
                .unwrap_or_default()
        ));
    }

    let mut terms: Vec<_> = engine.postings().keys().cloned().collect();
    terms.sort();

    for term in terms {
        if let Some(postings) = engine.postings().get(&term) {
            let mut postings = postings.clone();
            postings.sort_by_key(|posting| posting.doc_id);
            for posting in postings {
                let positions = posting
                    .positions
                    .iter()
                    .map(|position| position.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                lines.push(format!(
                    "POST\t{}\t{}\t{}",
                    escape_field(&term),
                    posting.doc_id,
                    positions
                ));
            }
        }
    }

    let mut data = lines.join("\n");
    data.push('\n');
    fs::write(path, data)?;
    Ok(())
}

pub(crate) fn load_engine(path: &Path) -> Result<SearchEngine, SearchError> {
    let contents = fs::read_to_string(path)?;
    let mut lines = contents.lines();

    let header = lines
        .next()
        .ok_or_else(|| SearchError::Parse("missing file header".to_string()))?;
    if header != MAGIC_HEADER_V1 && header != MAGIC_HEADER_V2 && header != MAGIC_HEADER_V3 {
        return Err(SearchError::Parse(format!(
            "unsupported file header: {header}"
        )));
    }
    let has_document_content = header == MAGIC_HEADER_V2 || header == MAGIC_HEADER_V3;
    let has_full_metadata = header == MAGIC_HEADER_V3;

    let avg_line = lines
        .next()
        .ok_or_else(|| SearchError::Parse("missing average-length line".to_string()))?;
    let avg_parts: Vec<&str> = avg_line.split('\t').collect();
    if avg_parts.len() != 2 || avg_parts[0] != "AVG" {
        return Err(SearchError::Parse(
            "invalid average-length line".to_string(),
        ));
    }
    let avg_doc_length = avg_parts[1]
        .parse::<f64>()
        .map_err(|_| SearchError::Parse("failed to parse average document length".to_string()))?;

    let mut documents = Vec::new();
    let mut postings: HashMap<String, Vec<Posting>> = HashMap::new();

    for line in lines {
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.first().copied() {
            Some("DOC") => {
                let expected_parts = if has_full_metadata {
                    8
                } else if has_document_content {
                    5
                } else {
                    4
                };
                if parts.len() != expected_parts {
                    return Err(SearchError::Parse(format!("invalid DOC line: {line}")));
                }
                let id = parts[1].parse::<usize>().map_err(|_| {
                    SearchError::Parse(format!("invalid document id in line: {line}"))
                })?;
                let path = unescape_field(parts[2])?;
                let length = parts[3].parse::<usize>().map_err(|_| {
                    SearchError::Parse(format!("invalid document length in line: {line}"))
                })?;
                let content = if has_document_content {
                    unescape_field(parts[4])?
                } else {
                    String::new()
                };
                let extension = if has_full_metadata {
                    let value = unescape_field(parts[5])?;
                    (!value.is_empty()).then_some(value)
                } else {
                    None
                };
                let title = if has_full_metadata {
                    unescape_field(parts[6])?
                } else {
                    String::new()
                };
                let modified_unix_timestamp_secs = if has_full_metadata && !parts[7].is_empty() {
                    Some(parts[7].parse::<u64>().map_err(|_| {
                        SearchError::Parse(format!("invalid modified timestamp in line: {line}"))
                    })?)
                } else {
                    None
                };
                documents.push(DocumentMeta::with_metadata(
                    id,
                    path,
                    length,
                    content,
                    extension,
                    title,
                    modified_unix_timestamp_secs,
                ));
            }
            Some("POST") => {
                if parts.len() != 4 {
                    return Err(SearchError::Parse(format!("invalid POST line: {line}")));
                }
                let term = unescape_field(parts[1])?;
                let doc_id = parts[2].parse::<usize>().map_err(|_| {
                    SearchError::Parse(format!("invalid posting doc id in line: {line}"))
                })?;
                let positions = if parts[3].is_empty() {
                    Vec::new()
                } else {
                    parts[3]
                        .split(',')
                        .map(|value| {
                            value.parse::<usize>().map_err(|_| {
                                SearchError::Parse(format!("invalid position in line: {line}"))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                postings
                    .entry(term)
                    .or_default()
                    .push(Posting { doc_id, positions });
            }
            Some(other) => {
                return Err(SearchError::Parse(format!("unknown record type: {other}")));
            }
            None => {}
        }
    }

    documents.sort_by_key(|doc| doc.id);
    for posting_list in postings.values_mut() {
        posting_list.sort_by_key(|posting| posting.doc_id);
    }

    Ok(SearchEngine::from_parts(
        documents,
        postings,
        avg_doc_length,
    ))
}

fn escape_field(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '%' => output.push_str("%25"),
            '\t' => output.push_str("%09"),
            '\n' => output.push_str("%0A"),
            '\r' => output.push_str("%0D"),
            _ => output.push(ch),
        }
    }
    output
}

fn unescape_field(input: &str) -> Result<String, SearchError> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }

        let a = chars
            .next()
            .ok_or_else(|| SearchError::Parse("truncated escape sequence".to_string()))?;
        let b = chars
            .next()
            .ok_or_else(|| SearchError::Parse("truncated escape sequence".to_string()))?;
        let code = [a, b].iter().collect::<String>();
        match code.as_str() {
            "25" => output.push('%'),
            "09" => output.push('\t'),
            "0A" => output.push('\n'),
            "0D" => output.push('\r'),
            _ => {
                return Err(SearchError::Parse(format!(
                    "unknown escape sequence: %{code}"
                )))
            }
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::SearchEngine;

    #[test]
    fn escape_roundtrip_handles_tabs_and_newlines() {
        let original = "hello\tworld\n100%";
        let escaped = escape_field(original);
        let decoded = unescape_field(&escaped).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let engine = SearchEngine::from_parts(
            vec![
                DocumentMeta::with_metadata(
                    0,
                    "a.txt",
                    3,
                    "rust rust bm25",
                    Some("txt".to_string()),
                    "Alpha Guide",
                    Some(1_700_000_000),
                ),
                DocumentMeta::with_metadata(
                    1,
                    "b.txt",
                    2,
                    "search engine",
                    Some("txt".to_string()),
                    "Beta Guide",
                    None,
                ),
            ],
            HashMap::from([
                (
                    "bm25".to_string(),
                    vec![Posting {
                        doc_id: 0,
                        positions: vec![2],
                    }],
                ),
                (
                    "engine".to_string(),
                    vec![Posting {
                        doc_id: 1,
                        positions: vec![1],
                    }],
                ),
                (
                    "rust".to_string(),
                    vec![Posting {
                        doc_id: 0,
                        positions: vec![0, 1],
                    }],
                ),
                (
                    "search".to_string(),
                    vec![Posting {
                        doc_id: 1,
                        positions: vec![0],
                    }],
                ),
            ]),
            2.5,
        );

        let path = std::env::temp_dir().join("mini_search_engine_roundtrip.idx");
        save_engine(&engine, &path).unwrap();
        let loaded = load_engine(&path).unwrap();

        assert_eq!(loaded.document_count(), 2);
        assert_eq!(loaded.vocabulary_size(), engine.vocabulary_size());
        assert_eq!(loaded.document(0).unwrap().content, "rust rust bm25");
        assert_eq!(
            loaded.document(0).unwrap().extension.as_deref(),
            Some("txt")
        );
        assert_eq!(loaded.document(0).unwrap().title, "Alpha Guide");
        assert_eq!(
            loaded.document(0).unwrap().modified_unix_timestamp_secs,
            Some(1_700_000_000)
        );
        let results = loaded.search("bm25", 5);
        let snippet = results[0].snippet.as_deref().unwrap();
        assert!(snippet.contains("[[bm25]]"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_engine_supports_legacy_v1_indexes() {
        let path = std::env::temp_dir().join("mini_search_engine_legacy.idx");
        fs::write(
            &path,
            "MSE1\nAVG\t2.000000000000\nDOC\t0\ta.txt\t2\nPOST\trust\t0\t0\n",
        )
        .unwrap();

        let loaded = load_engine(&path).unwrap();

        assert_eq!(loaded.document_count(), 1);
        assert_eq!(loaded.document(0).unwrap().content, "");
        assert_eq!(
            loaded.document(0).unwrap().extension.as_deref(),
            Some("txt")
        );
        assert_eq!(loaded.document(0).unwrap().title, "a");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_engine_supports_legacy_v2_indexes() {
        let path = std::env::temp_dir().join("mini_search_engine_legacy_v2.idx");
        fs::write(
            &path,
            "MSE2\nAVG\t2.000000000000\nDOC\t0\tguide.md\t2\t# Guide\nPOST\tguide\t0\t1\n",
        )
        .unwrap();

        let loaded = load_engine(&path).unwrap();

        assert_eq!(loaded.document_count(), 1);
        assert_eq!(loaded.document(0).unwrap().content, "# Guide");
        assert_eq!(loaded.document(0).unwrap().extension.as_deref(), Some("md"));
        assert_eq!(loaded.document(0).unwrap().title, "Guide");
        assert_eq!(
            loaded.document(0).unwrap().modified_unix_timestamp_secs,
            None
        );
        let _ = fs::remove_file(path);
    }
}
