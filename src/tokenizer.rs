#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionedToken {
    pub term: String,
    pub position: usize,
    pub start: usize,
    pub end: usize,
}

/// Tokenize text by:
///
/// - lowercasing
/// - splitting on non-alphanumeric chars
/// - and keeping only non-empty tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    tokenize_with_positions(text)
        .into_iter()
        .map(|token| token.term)
        .collect()
}

/// Tokenizes text and records positions plus byte offsets for highlighting.
pub fn tokenize_with_positions(text: &str) -> Vec<PositionedToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start = None;

    for (index, ch) in text.char_indices() {
        if ch.is_alphanumeric() {
            if start.is_none() {
                start = Some(index);
            }
            for lower in ch.to_lowercase() {
                current.push(lower);
            }
        } else if !current.is_empty() {
            let position = tokens.len();
            tokens.push(PositionedToken {
                term: std::mem::take(&mut current),
                position,
                start: start.take().unwrap_or(index),
                end: index,
            });
        }
    }

    if !current.is_empty() {
        let position = tokens.len();
        tokens.push(PositionedToken {
            term: current,
            position,
            start: start.unwrap_or(text.len()),
            end: text.len(),
        });
    }

    tokens
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

    #[test]
    fn offsets_capture_original_text_ranges() {
        let tokens = tokenize_with_positions("Rust search-engine");
        assert_eq!(
            &"Rust search-engine"[tokens[0].start..tokens[0].end],
            "Rust"
        );
        assert_eq!(
            &"Rust search-engine"[tokens[1].start..tokens[2].end],
            "search-engine"
        );
    }
}
