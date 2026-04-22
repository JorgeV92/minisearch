//! Query parsing.

use crate::tokenizer::tokenize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataField {
    Extension,
    Path,
    Title,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataFilter {
    pub field: MetadataField,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyTermQuery {
    pub term: String,
    pub max_distance: usize,
}

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
    pub optional_fuzzy_terms: Vec<FuzzyTermQuery>,
    pub required_fuzzy_terms: Vec<FuzzyTermQuery>,
    pub excluded_fuzzy_terms: Vec<FuzzyTermQuery>,
    pub required_metadata_filters: Vec<MetadataFilter>,
    pub excluded_metadata_filters: Vec<MetadataFilter>,
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
        if let Some(filter) = parse_metadata_filter(&token_text) {
            match prefix {
                '-' => parsed.excluded_metadata_filters.push(filter),
                _ => parsed.required_metadata_filters.push(filter),
            }
            continue;
        }
        if let Some(fuzzy_term) = parse_fuzzy_term(&token_text) {
            match prefix {
                '+' => parsed.required_fuzzy_terms.push(fuzzy_term),
                '-' => parsed.excluded_fuzzy_terms.push(fuzzy_term),
                _ => parsed.optional_fuzzy_terms.push(fuzzy_term),
            }
            continue;
        }

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

fn parse_fuzzy_term(token_text: &str) -> Option<FuzzyTermQuery> {
    let (base, suffix) = token_text.rsplit_once('~')?;
    if !suffix.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let mut normalized = tokenize(base);
    if normalized.len() != 1 {
        return None;
    }

    let max_distance = if suffix.is_empty() {
        1
    } else {
        suffix.parse::<usize>().ok()?
    };

    Some(FuzzyTermQuery {
        term: normalized.pop()?,
        max_distance,
    })
}

fn parse_metadata_filter(token_text: &str) -> Option<MetadataFilter> {
    let (field_name, raw_value) = token_text.split_once(':')?;
    let value = strip_optional_quotes(raw_value);
    if value.is_empty() {
        return None;
    }

    match field_name.to_ascii_lowercase().as_str() {
        "ext" => {
            let normalized = value.trim().trim_start_matches('.').to_ascii_lowercase();
            (!normalized.is_empty()).then_some(MetadataFilter {
                field: MetadataField::Extension,
                value: normalized,
            })
        }
        "path" => Some(MetadataFilter {
            field: MetadataField::Path,
            value: value.to_string(),
        }),
        "title" => Some(MetadataFilter {
            field: MetadataField::Title,
            value: value.to_string(),
        }),
        _ => None,
    }
}

fn strip_optional_quotes(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mixed_query() {
        let parsed = parse_query("+rust serch~1 \"search engine\" -java bm25");
        assert_eq!(parsed.required_terms, vec!["rust"]);
        assert_eq!(parsed.excluded_terms, vec!["java"]);
        assert_eq!(parsed.optional_terms, vec!["bm25"]);
        assert_eq!(
            parsed.optional_fuzzy_terms[0],
            FuzzyTermQuery {
                term: "serch".to_string(),
                max_distance: 1,
            }
        );
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

    #[test]
    fn parse_required_and_excluded_fuzzy_terms() {
        let parsed = parse_query("+tokenzier~2 -jav~1 rust~");
        assert_eq!(
            parsed.required_fuzzy_terms[0],
            FuzzyTermQuery {
                term: "tokenzier".to_string(),
                max_distance: 2,
            }
        );
        assert_eq!(
            parsed.excluded_fuzzy_terms[0],
            FuzzyTermQuery {
                term: "jav".to_string(),
                max_distance: 1,
            }
        );
        assert_eq!(
            parsed.optional_fuzzy_terms[0],
            FuzzyTermQuery {
                term: "rust".to_string(),
                max_distance: 1,
            }
        );
    }

    #[test]
    fn parse_metadata_filters() {
        let parsed = parse_query("ext:rs +path:src/ -title:generated");
        assert_eq!(
            parsed.required_metadata_filters,
            vec![
                MetadataFilter {
                    field: MetadataField::Extension,
                    value: "rs".to_string(),
                },
                MetadataFilter {
                    field: MetadataField::Path,
                    value: "src/".to_string(),
                }
            ]
        );
        assert_eq!(
            parsed.excluded_metadata_filters,
            vec![MetadataFilter {
                field: MetadataField::Title,
                value: "generated".to_string(),
            }]
        );
    }
}
