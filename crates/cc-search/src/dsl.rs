//! Search DSL parser: extracts structured filters from query strings.
//!
//! Supported filter keys (case-insensitive):
//! - `kind:` — filter by symbol kind (e.g. `kind:function`, `kind:"type alias"`)
//! - `lang:` — filter by language (e.g. `lang:python`, `lang:rust`)
//! - `path:` — filter by path prefix (e.g. `path:src/api`)
//! - `name:` — filter/boost by symbol name (e.g. `name:handle_request`)

/// Parsed search query with extracted structured filters.
#[derive(Debug, Clone, Default)]
pub struct ParsedQuery {
    /// Remaining free-text after filter extraction.
    pub text: String,
    /// `kind:` filter — matches against `SymbolKind::as_str()` (case-insensitive).
    pub kind_filter: Option<String>,
    /// `lang:` filter — resolved via `Language::from_name()`.
    pub lang_filter: Option<String>,
    /// `path:` filter — used as path prefix.
    pub path_filter: Option<String>,
    /// `name:` filter — matches against symbol name.
    pub name_filter: Option<String>,
}

/// Known filter keys.
const FILTER_KEYS: &[&str] = &["kind", "lang", "path", "name"];

/// Parse a raw search query, extracting `kind:`, `lang:`, `path:`, `name:` filters.
/// Remaining text (after filter removal) becomes `text`.
/// Supports quoted values: `kind:"type alias"`.
pub fn parse_search_dsl(raw: &str) -> ParsedQuery {
    let mut result = ParsedQuery::default();
    let mut remaining_tokens: Vec<&str> = Vec::new();

    let input = raw.trim();
    if input.is_empty() {
        return result;
    }

    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut pos = 0;

    while pos < len {
        // Skip whitespace
        if chars[pos].is_whitespace() {
            pos += 1;
            continue;
        }

        // Try to match a filter key
        let mut matched_key: Option<&str> = None;
        for &key in FILTER_KEYS {
            let key_colon = format!("{}:", key);
            let key_colon_len = key_colon.len();
            if pos + key_colon_len <= len {
                let candidate: String = chars[pos..pos + key_colon_len].iter().collect();
                if candidate.eq_ignore_ascii_case(&key_colon) {
                    matched_key = Some(key);
                    pos += key_colon_len;
                    break;
                }
            }
        }

        if let Some(key) = matched_key {
            // Extract value — either quoted or unquoted
            let value = if pos < len && chars[pos] == '"' {
                // Quoted value: consume until closing quote
                pos += 1; // skip opening quote
                let start = pos;
                while pos < len && chars[pos] != '"' {
                    pos += 1;
                }
                let val: String = chars[start..pos].iter().collect();
                if pos < len && chars[pos] == '"' {
                    pos += 1; // skip closing quote
                }
                val
            } else {
                // Unquoted value: consume until whitespace
                let start = pos;
                while pos < len && !chars[pos].is_whitespace() {
                    pos += 1;
                }
                chars[start..pos].iter().collect()
            };

            if !value.is_empty() {
                match key {
                    "kind" => result.kind_filter = Some(value),
                    "lang" => result.lang_filter = Some(value),
                    "path" => result.path_filter = Some(value),
                    "name" => result.name_filter = Some(value),
                    _ => {}
                }
            }
        } else {
            // Regular token: consume until whitespace
            let start = pos;
            while pos < len && !chars[pos].is_whitespace() {
                pos += 1;
            }
            let token: &str = &input[byte_offset(&chars, start)..byte_offset(&chars, pos)];
            remaining_tokens.push(token);
        }
    }

    result.text = remaining_tokens.join(" ");
    result
}

/// Convert a char index to a byte offset in the original string.
fn byte_offset(chars: &[char], char_idx: usize) -> usize {
    chars[..char_idx].iter().map(|c| c.len_utf8()).sum()
}

/// Check if a file extension / language name matches the lang filter value.
/// Uses `Language::from_name` for lenient matching.
pub fn matches_language(file_path: &str, lang_filter: &str) -> bool {
    let filter_lang = cc_model::Language::from_name(lang_filter);
    if filter_lang == cc_model::Language::Unknown {
        // Fallback: direct substring match on file extension
        let ext = file_path.rsplit('.').next().unwrap_or("");
        return ext.eq_ignore_ascii_case(lang_filter);
    }
    // Match by file extension
    let ext = file_path.rsplit('.').next().unwrap_or("");
    let file_lang = cc_model::Language::from_extension(ext);
    file_lang == filter_lang
}

/// Check if a `SymbolKind` matches the kind filter string (case-insensitive).
/// Normalizes spaces/underscores for flexible matching (e.g. "type alias" == "type_alias").
pub fn matches_kind(symbol_kind: &cc_model::symbol::SymbolKind, kind_filter: &str) -> bool {
    let kind_str = symbol_kind.as_str();
    let normalized_filter = kind_filter.to_lowercase().replace(' ', "_");
    let normalized_kind = kind_str.to_lowercase();
    normalized_kind == normalized_filter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_filters() {
        let q = parse_search_dsl("kind:function handle_request");
        assert_eq!(q.kind_filter, Some("function".into()));
        assert_eq!(q.text, "handle_request");
    }

    #[test]
    fn test_quoted_value() {
        let q = parse_search_dsl("kind:\"type alias\" MyType");
        assert_eq!(q.kind_filter, Some("type alias".into()));
        assert_eq!(q.text, "MyType");
    }

    #[test]
    fn test_multiple_filters() {
        let q = parse_search_dsl("lang:python path:src/api kind:class UserService");
        assert_eq!(q.lang_filter, Some("python".into()));
        assert_eq!(q.path_filter, Some("src/api".into()));
        assert_eq!(q.kind_filter, Some("class".into()));
        assert_eq!(q.text, "UserService");
    }

    #[test]
    fn test_no_filters() {
        let q = parse_search_dsl("hello world search");
        assert!(q.kind_filter.is_none());
        assert!(q.lang_filter.is_none());
        assert!(q.path_filter.is_none());
        assert!(q.name_filter.is_none());
        assert_eq!(q.text, "hello world search");
    }

    #[test]
    fn test_case_insensitive_keys() {
        let q = parse_search_dsl("Kind:function Lang:Rust search_term");
        assert_eq!(q.kind_filter, Some("function".into()));
        assert_eq!(q.lang_filter, Some("Rust".into()));
        assert_eq!(q.text, "search_term");
    }

    #[test]
    fn test_name_filter() {
        let q = parse_search_dsl("name:handle_request kind:function");
        assert_eq!(q.name_filter, Some("handle_request".into()));
        assert_eq!(q.kind_filter, Some("function".into()));
        assert!(q.text.is_empty());
    }

    #[test]
    fn test_empty_input() {
        let q = parse_search_dsl("");
        assert!(q.text.is_empty());
        assert!(q.kind_filter.is_none());
    }

    #[test]
    fn test_only_filters_no_text() {
        let q = parse_search_dsl("kind:class lang:python");
        assert_eq!(q.kind_filter, Some("class".into()));
        assert_eq!(q.lang_filter, Some("python".into()));
        assert!(q.text.is_empty());
    }

    #[test]
    fn test_matches_language_python() {
        assert!(matches_language("src/app/handler.py", "python"));
        assert!(matches_language("src/app/handler.py", "Python"));
        assert!(matches_language("src/app/handler.py", "py"));
        assert!(!matches_language("src/app/handler.rs", "python"));
    }

    #[test]
    fn test_matches_kind() {
        use cc_model::symbol::SymbolKind;
        assert!(matches_kind(&SymbolKind::Function, "function"));
        assert!(matches_kind(&SymbolKind::Function, "Function"));
        assert!(matches_kind(&SymbolKind::TypeAlias, "type_alias"));
        assert!(matches_kind(&SymbolKind::TypeAlias, "type alias"));
        assert!(!matches_kind(&SymbolKind::Function, "class"));
    }
}
