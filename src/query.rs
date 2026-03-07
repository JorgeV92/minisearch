use std::os::unix::process::parent_id;

/// Query parsing.
 
use crate::tokenizer::tokenize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhraseQuery {
    pub terms: Vec<String>
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedQuery {
    pub optional_terms: Vec<String>,
    pub required_terms: Vec<String>,
    pub excluded_terms: Vec<String>,
    pub phrases: Vec<PhraseQuery>,
}

pub fn parse_query(input: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        let prefix = if chars[i] == '+' || chars[i] == '-' {
            let prefix = chars[i];
            i += 1;
            prefix
        } else {
            '\0'
        };

        if i < chars.len() && chars[i] == '"' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let phrase_text: String = chars[start..i].iter().collect();
            let terms = tokenize(&phrase_text);
            if !terms.is_empty() && prefix != '-' {
                parsed.phrases.push(PhraseQuery { terms });
            }
            if i < chars.len() && chars[i] == '"' {
                i += 1;
            }
            continue;
        }

        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let token_text: String = chars[start..i].iter().collect();
        let normalized = tokenize(&token_text);
        if normalized.is_empty() {
            continue;
        }

        for term in normalized {
            match prefix {
                '+' => parsed.required_terms.push(term),
                '-' => parsed.excluded_terms.push(term),
                _ => parsed.optional_terms.push(term),
            }
        }
    }

    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mixed_query() {
        let parsed = parse_query("+rust \"search engine\" -java bm25");
        assert_eq!(parsed.required_terms, vec!["rust"]);
        assert_eq!(parsed.excluded_terms, vec!["java"]);
        assert_eq!(parsed.optional_terms, vec!["bm25"]);
        assert_eq!(parsed.phrases[0].terms, vec!["search", "engine"]);
    }
}