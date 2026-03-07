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

/// Tokenizes text and records positions for phrase matching.
pub fn tokenize_with_positions(text: &str) -> Vec<PositionedToken> {
    tokenize(text)
        .into_iter()
        .enumerate()
        .map(|(position, term)| PositionedToken { term, position })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenization_normalizes_case_and_punctuation() {
        let tokens = tokenize("Rust, Search-Engine! 101");
        assert_eq!(tokens, vec!["rust", "search", "engine", "101"]);
    }

    #[test]
    fn positions_are_montonic() {
        let tokens = tokenize_with_positions("alpha beta beta");
        assert_eq!(tokens[0].term, "alpha");
        assert_eq!(tokens[0].position, 0);
        assert_eq!(tokens[2].position, 2);
    }
}