//! Index persistence.
//!
//! The on-disk format is intentionally simple and human-readable:
//!
//! ```text
//! MSE1
//! AVG\t3.500000
//! DOC\t0\tREADME.md\t120
//! POST\trust\t0\t0,9,42
//! ```
//!

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::document::DocumentMeta;
use crate::index::{Posting, SearchEngine, SearchError};

const MAGIC_HEADER: &str = "MSE1";

pub(crate) fn save_engine(engine: &SearchEngine, path: &Path) -> Result<(), SearchError> {
    let mut lines = Vec::new();
    lines.push(MAGIC_HEADER.to_string());
    lines.push(format!("AVG\t{:.12}", engine.average_document_length()));

    for doc in engine.documents() {
        lines.push(format!(
            "DOC\t{}\t{}\t{}",
            doc.id,
            escape_field(&doc.path),
            doc.length
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


