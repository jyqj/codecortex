//! Conservative target hints for the additive exact-name bonus only.
//! This is an explicit syntax recognizer, not a natural-language intent model.
use cc_model::symbol::SymbolKind;

use crate::dsl::ParsedQuery;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QueryTarget {
    /// No explicit evidence: retain the historical token-based fallback.
    Ambiguous,
    Named {
        name: String,
        kind: Option<SymbolKind>,
    },
    /// A container is mentioned, but no member name was requested.
    ContainerMethods,
}

impl QueryTarget {
    pub(crate) fn parse(query: &ParsedQuery) -> Self {
        // Reuse the caller's existing explicit target filter, before prose.
        if let Some(name) = &query.name_filter {
            return Self::Named {
                name: name.to_lowercase(),
                kind: None,
            };
        }
        let text = query.text.trim();
        if let Some(name) = member_name(text) {
            return Self::Named {
                name: name.to_lowercase(),
                kind: None,
            };
        }
        let words: Vec<_> = text.split_whitespace().collect();
        if let [methods, on, container] = words.as_slice() {
            if methods.eq_ignore_ascii_case("methods")
                && on.eq_ignore_ascii_case("on")
                && identifier(container)
            {
                return Self::ContainerMethods;
            }
        }
        // Whole-query grammar prevents a keyword elsewhere in prose from
        // being mistaken for the user's target. Unknown syntax stays ambiguous.
        let (kind_word, name) = match words.as_slice() {
            [kind, name] => (*kind, *name),
            [kind, name, on, container]
                if kind.eq_ignore_ascii_case("method")
                    && on.eq_ignore_ascii_case("on")
                    && identifier(container) =>
            {
                (*kind, *name)
            }
            _ => return Self::Ambiguous,
        };
        let kind = match kind_word.to_ascii_lowercase().as_str() {
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "type" => SymbolKind::TypeAlias,
            "function" => SymbolKind::Function,
            "method" => SymbolKind::Method,
            _ => return Self::Ambiguous,
        };
        if !identifier(name) {
            return Self::Ambiguous;
        }
        Self::Named {
            name: name.to_lowercase(),
            kind: Some(kind),
        }
    }

    pub(crate) fn permits(&self, name: &str, kind: Option<&str>, legacy_tokens: &[String]) -> bool {
        let lower = name.to_lowercase();
        match self {
            Self::Ambiguous => legacy_tokens.contains(&lower),
            Self::Named {
                name,
                kind: target_kind,
            } => name == &lower && target_kind.is_none_or(|k| kind == Some(k.as_str())),
            Self::ContainerMethods => false,
        }
    }
}

fn identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

/// Only pure `::` chains and the narrow `(*Receiver).member` form give a hint.
/// Bare dotted strings can be filenames, modules or members: no extension,
/// capitalization or filesystem evidence can make them explicit here.
/// This does not resolve qualifiers or infer that every member is callable.
fn member_name(text: &str) -> Option<&str> {
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    let text = if let Some(rest) = text.strip_prefix("(*") {
        let (receiver, member) = rest.split_once(").")?;
        return (identifier(receiver) && identifier(member)).then_some(member);
    } else {
        text
    };
    let parts: Vec<_> = text.split("::").collect();
    (parts.len() > 1 && parts.iter().all(|part| identifier(part))).then(|| *parts.last().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_v2_independent_matrix() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../artifacts/controls/query-target-v2-20261004/matrix.json"
        ))
        .unwrap();
        for row in rows.as_array().unwrap() {
            let query = row["query"].as_str().unwrap();
            let target = QueryTarget::parse(&crate::dsl::parse_search_dsl(query));
            let actual = match target {
                QueryTarget::Ambiguous => "fallback".into(),
                QueryTarget::ContainerMethods => "no-name-boost".into(),
                QueryTarget::Named { name, kind } => {
                    format!("named:{name}:{}", kind.map_or("any", |k| k.as_str()))
                }
            };
            assert_eq!(
                actual.to_lowercase(),
                row["expected_model"].as_str().unwrap().to_lowercase(),
                "{query}"
            );
        }
    }

    #[test]
    fn historical_v1_dot_expectations_are_explicitly_superseded() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../artifacts/controls/query-target-20261004/matrix.json"
        ))
        .unwrap();
        let mut superseded = Vec::new();
        for row in rows.as_array().unwrap() {
            let query = row["query"].as_str().unwrap();
            if matches!(
                row["id"].as_str(),
                Some("qualified-dot" | "qualified-field")
            ) {
                assert!(row["expected_model"]
                    .as_str()
                    .unwrap()
                    .starts_with("named:"));
                assert_eq!(
                    QueryTarget::parse(&crate::dsl::parse_search_dsl(query)),
                    QueryTarget::Ambiguous
                );
                superseded.push(row["id"].as_str().unwrap());
            }
        }
        assert_eq!(superseded, vec!["qualified-dot", "qualified-field"]);
    }

    #[test]
    fn filename_counterexamples_retain_legacy_name_eligibility() {
        for (query, container, suffix) in [
            ("Beacon.spec.py", "Beacon", "py"),
            ("Vessel.yaml", "Vessel", "yaml"),
        ] {
            let target = QueryTarget::parse(&crate::dsl::parse_search_dsl(query));
            assert_eq!(target, QueryTarget::Ambiguous);
            let tokens = cc_db::fts::tokenize_codeish(query);
            assert!(target.permits(container, Some("class"), &tokens));
            assert!(target.permits(suffix, Some("function"), &tokens));
        }
        let tokens = vec!["beacon".into(), "pulse".into()];
        for query in ["lights::Beacon::pulse", "(*Beacon).pulse"] {
            let target = QueryTarget::parse(&crate::dsl::parse_search_dsl(query));
            assert!(target.permits("pulse", Some("method"), &tokens));
            assert!(!target.permits("Beacon", Some("class"), &tokens));
        }
    }

    #[test]
    fn target_kind_is_evidence_not_global_type_demotion() {
        let target = QueryTarget::parse(&crate::dsl::parse_search_dsl("method Ignite on Lantern"));
        let tokens = vec!["ignite".into(), "lantern".into()];
        assert!(target.permits("Ignite", Some("method"), &tokens));
        assert!(!target.permits("Ignite", Some("class"), &tokens));
        assert!(!target.permits("Lantern", Some("class"), &tokens));
        assert!(!target.permits("Ignite", None, &tokens));
        let direct = QueryTarget::parse(&crate::dsl::parse_search_dsl("class Ignite"));
        assert!(direct.permits("Ignite", Some("class"), &tokens));
        assert!(!direct.permits("Ignite", Some("method"), &tokens));
    }

    #[test]
    fn conservative_fallback_and_invalid_syntax() {
        for query in [
            "",
            "methods on",
            "method Ignite on Lantern extra",
            "class 42",
            "A..B",
            "(*A).B.C",
            "explain class Lantern initialization",
        ] {
            assert_eq!(
                QueryTarget::parse(&crate::dsl::parse_search_dsl(query)),
                QueryTarget::Ambiguous,
                "{query}"
            );
        }
        assert!(QueryTarget::Ambiguous.permits("Lantern", Some("class"), &["lantern".into()]));
        assert!(!QueryTarget::ContainerMethods.permits(
            "Lantern",
            Some("method"),
            &["lantern".into()]
        ));
    }
}
