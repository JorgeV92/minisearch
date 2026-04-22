//! Document metadata used by the search engine.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentMeta {
    pub id: usize,
    pub path: String,
    pub length: usize,
    pub content: String,
    pub extension: Option<String>,
    pub title: String,
    pub modified_unix_timestamp_secs: Option<u64>,
}

impl DocumentMeta {
    pub fn new(
        id: usize,
        path: impl Into<String>,
        length: usize,
        content: impl Into<String>,
    ) -> Self {
        Self::with_metadata(id, path, length, content, None, String::new(), None)
    }

    pub fn with_metadata(
        id: usize,
        path: impl Into<String>,
        length: usize,
        content: impl Into<String>,
        extension: Option<String>,
        title: impl Into<String>,
        modified_unix_timestamp_secs: Option<u64>,
    ) -> Self {
        let path = path.into();
        let content = content.into();
        let extension = normalize_extension(extension.or_else(|| infer_extension(&path)));
        let title = {
            let title = title.into();
            if title.trim().is_empty() {
                infer_title(&path, &content)
            } else {
                title
            }
        };

        Self {
            id,
            path,
            length,
            content,
            extension,
            title,
            modified_unix_timestamp_secs,
        }
    }
}

fn infer_extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
}

fn normalize_extension(extension: Option<String>) -> Option<String> {
    extension.and_then(|value| {
        let normalized = value.trim().trim_start_matches('.').to_ascii_lowercase();
        (!normalized.is_empty()).then_some(normalized)
    })
}

fn infer_title(path: &str, content: &str) -> String {
    if let Some(line) = content.lines().map(str::trim).find(|line| !line.is_empty()) {
        if let Some(stripped) = line.strip_prefix('#') {
            let heading = stripped.trim_start_matches('#').trim();
            if !heading.is_empty() {
                return heading.to_string();
            }
        }
    }

    if let Some(stem) = Path::new(path).file_stem().and_then(|value| value.to_str()) {
        if !stem.is_empty() {
            return stem.to_string();
        }
    }

    path.to_string()
}
