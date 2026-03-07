#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionedToken {
    pub term: String,
    pub position: usize,
}

/// Tokenize text by:
/// 
/// - lowercasing
/// - splitting on non-alphanumeric chars
/// - and keeping only non-empty tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

