//! Document metadata used by the search engine.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentMeta {
    pub id: usize,
    pub path: String,
    pub length: usize,
}

impl DocumentMeta {
    pub fn new(id: usize, path: impl Into<String>, length: usize) -> Self {
        Self {
            id, 
            path: path.into(),
            length,
        }
    }
}

