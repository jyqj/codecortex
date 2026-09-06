//! FTS5 query building helpers.

use std::sync::LazyLock;

static FTS_TOKEN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[\p{L}\p{N}_]+").unwrap());

/// Sanitize a user query into FTS5 match syntax.
/// Extracts alphanumeric tokens and joins with OR (max 12 tokens).
pub fn sanitize_fts_query(query: &str) -> String {
    let tokens = select_query_tokens(query, 12);
    if tokens.is_empty() {
        return r#""""#.to_string();
    }
    // Quote every token: identifiers named OR/AND/NOT must remain literals.
    tokens
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Expand a code query by splitting camelCase and snake_case tokens.
pub fn expand_query_text(query: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for word in query.split_whitespace() {
        tokens.push(word.to_string());
        // Split camelCase
        let camel_parts = split_camel_case(word);
        if camel_parts.len() > 1 {
            for part in &camel_parts {
                let lower = part.to_lowercase();
                if !tokens.iter().any(|t| t.eq_ignore_ascii_case(&lower)) {
                    tokens.push(lower);
                }
            }
        }
        // Split snake_case
        if word.contains('_') {
            for part in word.split('_') {
                if part.len() >= 2 {
                    let lower = part.to_lowercase();
                    if !tokens.iter().any(|t| t.eq_ignore_ascii_case(&lower)) {
                        tokens.push(lower);
                    }
                }
            }
        }
    }
    tokens.join(" ")
}

/// Split a camelCase or PascalCase identifier into parts.
fn split_camel_case(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..bytes.len() {
        if bytes[i].is_ascii_uppercase()
            && (bytes[i - 1].is_ascii_lowercase()
                || (bytes[i - 1].is_ascii_uppercase()
                    && bytes.get(i + 1).is_some_and(u8::is_ascii_lowercase)))
        {
            parts.push(&s[start..i]);
            start = i;
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Tokenize text in a code-aware way (for overlap scoring).
pub fn tokenize_codeish(text: &str) -> Vec<String> {
    FTS_TOKEN_RE
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

/// Bounded, deterministic query planning. Deduplicate BEFORE the budget; reserve
/// priority for code-shaped identifiers anywhere in the query rather than just
/// taking its prose prefix. This is a heuristic, not a learned relevance model.
pub fn select_query_tokens(query: &str, budget: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut tokens = Vec::new();
    for (index, m) in FTS_TOKEN_RE.find_iter(query).enumerate() {
        let t = m.as_str();
        let lower = t.to_lowercase();
        if !seen.insert(lower.clone()) {
            continue;
        }
        let bytes = t.as_bytes();
        let code = t.contains('_')
            || t.chars().any(|c| c.is_numeric())
            || bytes
                .windows(2)
                .any(|p| p[0].is_ascii_lowercase() && p[1].is_ascii_uppercase());
        let quoted =
            m.start() > 0 && matches!(query.as_bytes()[m.start() - 1], b'`' | b'"' | b'\'');
        let boilerplate = matches!(
            lower.as_str(),
            "the"
                | "a"
                | "an"
                | "is"
                | "of"
                | "to"
                | "in"
                | "and"
                | "or"
                | "please"
                | "find"
                | "where"
                | "does"
                | "how"
                | "this"
                | "that"
                | "for"
                | "with"
                | "can"
                | "you"
        );
        let priority = if quoted {
            3
        } else if code {
            2
        } else if boilerplate {
            0
        } else {
            1
        };
        tokens.push((priority, index, t.to_owned()));
    }
    tokens.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    tokens.into_iter().take(budget).map(|(_, _, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_sanitize_basic() {
        assert_eq!(sanitize_fts_query("hello world"), r#""hello" OR "world""#);
        assert_eq!(sanitize_fts_query(""), r#""""#);
        assert_eq!(sanitize_fts_query("foo_bar"), r#""foo_bar""#);
    }

    #[test]
    fn camel_case_split() {
        assert_eq!(split_camel_case("handleRequest"), vec!["handle", "Request"]);
        assert_eq!(split_camel_case("foo"), vec!["foo"]);
    }

    #[test]
    fn expand_query() {
        let expanded = expand_query_text("handleRequest");
        assert!(expanded.contains("handle"));
        assert!(expanded.contains("request"));
    }

    #[test]
    fn expand_query_snake_case() {
        let expanded = expand_query_text("get_user_name");
        assert!(expanded.contains("get"));
        assert!(expanded.contains("user"));
        assert!(expanded.contains("name"));
    }
}
#[cfg(test)]
mod query_contract_tests {
    use super::*;
    #[test]
    fn code_identifier_after_prose_prefix_survives_budget() {
        let q="please find where the application does a thing in this module and determine how it works with validatePaymentToken";
        assert_eq!(select_query_tokens(q, 1), ["validatePaymentToken"]);
        assert!(sanitize_fts_query(q).contains("validatePaymentToken"));
    }
    #[test]
    fn repeated_words_do_not_spend_candidate_budget() {
        assert_eq!(select_query_tokens("x x x X wanted", 2), ["x", "wanted"]);
    }
    #[test]
    fn acronym_and_unicode_are_not_damaged() {
        assert_eq!(split_camel_case("HTTPServer"), ["HTTP", "Server"]);
        assert_eq!(
            select_query_tokens("支付 校验 Привет", 3),
            ["支付", "校验", "Привет"]
        );
    }
    #[test]
    fn fts_operators_are_literal_terms() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE x USING fts5(text); INSERT INTO x VALUES('OR');")
            .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM x WHERE x MATCH ?1",
                [sanitize_fts_query("OR")],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
}
