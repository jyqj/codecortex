//! Extract candidate symbol names from natural language task descriptions.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "the", "and", "for", "not", "with", "has", "get", "set", "all", "any", "new", "old", "add",
        "can", "may", "use", "run", "let", "var", "try", "end", "top", "but", "are", "was", "how",
        "its", "our", "out", "now", "way", "did", "too", "own", "put", "say", "she", "his", "him",
        "her", "who", "will", "from", "that", "this", "when", "what", "make", "like", "been",
        "have", "just", "more", "some", "than", "them", "then", "into", "fix", "bug", "need",
        "want", "should", "could", "would",
    ]
    .into_iter()
    .collect()
});

static RE_CAMEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Z][a-z]+(?:[A-Z][a-z0-9]+)+").unwrap());

static RE_SNAKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)+").unwrap());

static RE_SCREAMING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+").unwrap());

static RE_DOTTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)+").unwrap());

/// Extract candidate symbol names from a natural language task description.
///
/// Returns up to 20 candidates sorted by length descending (longer = more specific).
pub fn extract_candidate_symbols(text: &str) -> Vec<String> {
    let mut candidates = HashSet::new();

    for m in RE_CAMEL.find_iter(text) {
        let s = m.as_str();
        if !is_stop_word(s) {
            candidates.insert(s.to_string());
        }
    }

    for m in RE_SNAKE.find_iter(text) {
        let s = m.as_str();
        if !is_stop_word(s) {
            candidates.insert(s.to_string());
        }
    }

    for m in RE_SCREAMING.find_iter(text) {
        let s = m.as_str();
        if !is_stop_word(s) {
            candidates.insert(s.to_string());
        }
    }

    for m in RE_DOTTED.find_iter(text) {
        let s = m.as_str();
        if !is_stop_word(s) {
            candidates.insert(s.to_string());
        }
    }

    let mut result: Vec<String> = candidates.into_iter().collect();
    result.sort_by(|a, b| b.len().cmp(&a.len()));
    result.truncate(20);
    result
}

fn is_stop_word(s: &str) -> bool {
    STOP_WORDS.contains(s.to_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_case() {
        let syms = extract_candidate_symbols("Fix the UserService authentication bug");
        assert!(syms.contains(&"UserService".to_string()));
    }

    #[test]
    fn test_snake_case() {
        let syms = extract_candidate_symbols("Refactor the build_index function");
        assert!(syms.contains(&"build_index".to_string()));
    }

    #[test]
    fn test_screaming_snake() {
        let syms = extract_candidate_symbols("Increase MAX_RETRIES to 5");
        assert!(syms.contains(&"MAX_RETRIES".to_string()));
    }

    #[test]
    fn test_dotted_name() {
        let syms = extract_candidate_symbols("Update config.search settings");
        assert!(syms.contains(&"config.search".to_string()));
    }

    #[test]
    fn test_mixed() {
        let syms = extract_candidate_symbols(
            "Fix HttpClient retry logic in build_index and MAX_RETRIES constant",
        );
        assert!(syms.contains(&"HttpClient".to_string()));
        assert!(syms.contains(&"build_index".to_string()));
        assert!(syms.contains(&"MAX_RETRIES".to_string()));
    }

    #[test]
    fn test_stop_words_filtered() {
        let syms = extract_candidate_symbols("fix this bug and add new feature");
        assert!(syms.is_empty());
    }

    #[test]
    fn test_max_20() {
        // Generate enough candidates to verify truncation
        let input = "AaaBbb CccDdd EeeFff GggHhh IiiJjj KkkLll MmmNnn OooPpp QqqRrr SssTtt \
                      UuuVvv WwwXxx YyyZzz AaaXxx BbbYyy CccZzz DddAaa EeeBbb FffCcc GggDdd HhhEee";
        let syms = extract_candidate_symbols(input);
        assert!(syms.len() <= 20);
    }

    #[test]
    fn test_dedup() {
        let syms = extract_candidate_symbols("UserService calls UserService again in UserService");
        let count = syms.iter().filter(|s| *s == "UserService").count();
        assert_eq!(count, 1);
    }
}
