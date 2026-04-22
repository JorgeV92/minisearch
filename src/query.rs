//! Query parsing.

use crate::tokenizer::tokenize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhraseQuery {
    pub terms: Vec<String>,
    pub slop: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedQuery {
    pub optional_terms: Vec<String>,
    pub required_terms: Vec<String>,
    pub excluded_terms: Vec<String>,
    pub phrases: Vec<PhraseQuery>,
    pub required_phrases: Vec<PhraseQuery>,
    pub excluded_phrases: Vec<PhraseQuery>,
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
            if i < chars.len() && chars[i] == '"' {
                i += 1;
            }
            let slop = parse_phrase_slop(&chars, &mut i);
            let terms = tokenize(&phrase_text);
            if !terms.is_empty() {
                let phrase = PhraseQuery { terms, slop };
                match prefix {
                    '+' => parsed.required_phrases.push(phrase),
                    '-' => parsed.excluded_phrases.push(phrase),
                    _ => parsed.phrases.push(phrase),
                }
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

fn parse_phrase_slop(chars: &[char], index: &mut usize) -> usize {
    if *index >= chars.len() || chars[*index] != '~' {
        return 0;
    }

    *index += 1;
    let mut slop = 0usize;
    while *index < chars.len() {
        let Some(digit) = chars[*index].to_digit(10) else {
            break;
        };
        slop = slop.saturating_mul(10).saturating_add(digit as usize);
        *index += 1;
    }

    slop
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
        assert_eq!(
            parsed.phrases[0],
            PhraseQuery {
                terms: vec!["search".to_string(), "engine".to_string()],
                slop: 0,
            }
        );
    }

    #[test]
    fn parse_required_and_excluded_phrases() {
        let parsed = parse_query("+\"search engine\" -\"toy example\"");
        assert!(parsed.phrases.is_empty());
        assert_eq!(
            parsed.required_phrases[0],
            PhraseQuery {
                terms: vec!["search".to_string(), "engine".to_string()],
                slop: 0,
            }
        );
        assert_eq!(
            parsed.excluded_phrases[0],
            PhraseQuery {
                terms: vec!["toy".to_string(), "example".to_string()],
                slop: 0,
            }
        );
    }

    #[test]
    fn parse_proximity_phrases() {
        let parsed = parse_query("+\"distributed systems\"~3 -\"toy example\"~1");
        assert_eq!(
            parsed.required_phrases[0],
            PhraseQuery {
                terms: vec!["distributed".to_string(), "systems".to_string()],
                slop: 3,
            }
        );
        assert_eq!(
            parsed.excluded_phrases[0],
            PhraseQuery {
                terms: vec!["toy".to_string(), "example".to_string()],
                slop: 1,
            }
        );
    }
}
