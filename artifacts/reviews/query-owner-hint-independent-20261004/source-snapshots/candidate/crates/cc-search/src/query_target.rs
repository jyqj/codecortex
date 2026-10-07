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

// Bounded English owner-role context, separate from explicit target identity.
// This does not infer general natural-language intent or resolve receivers.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct NameBonusContext {
    contextual_owner: Option<String>,
}

impl NameBonusContext {
    pub(crate) fn parse(query: &ParsedQuery, target: &QueryTarget) -> Self {
        // Explicit target/kind instructions always outrank this soft hint.
        if !matches!(target, QueryTarget::Ambiguous) || query.kind_filter.is_some() {
            return Self::default();
        }
        let text = query.text.trim().trim_end_matches(['?', '.', '!']);
        let words: Vec<_> = text.split_whitespace().collect();
        // Conservative abstention for coordinated/comparative queries. This
        // is a documented finite grammar boundary, not a universal detector.
        if words.iter().any(|word| comparison_or_coordination(word)) {
            return Self::default();
        }
        let owner = match words.as_slice() {
            [head, owner, role, tail @ ..]
                if question_head(head) && member_role(role) && !tail.is_empty() =>
            {
                Some(*owner)
            }
            [head, role, relation, owner, tail @ ..]
                if question_head(head)
                    && member_role(role)
                    && matches!(relation.to_ascii_lowercase().as_str(), "on" | "of" | "in")
                    && !tail.is_empty() =>
            {
                Some(*owner)
            }
            _ => None,
        };
        Self {
            contextual_owner: owner
                .filter(|owner| identifier(owner))
                .map(str::to_lowercase),
        }
    }

    pub(crate) fn permits(&self, name: &str, kind: Option<&str>) -> bool {
        let Some(owner) = self.contextual_owner.as_deref() else {
            return true;
        };
        // Do not hide hits or change their taxonomy. Withhold only this
        // named container's additive bonus. Unknown kinds keep fallback;
        // an unrelated type and a same-spelled method keep eligibility.
        name.to_lowercase() != owner
            || !matches!(
                kind,
                Some("class" | "interface" | "type_alias" | "enum" | "module" | "namespace")
            )
    }
}

fn question_head(word: &str) -> bool {
    matches!(word.to_ascii_lowercase().as_str(), "which" | "what")
}

fn member_role(word: &str) -> bool {
    // `api` does NOT imply a callable kind. Accepting its owner reading is
    // the main product decision in this review candidate.
    matches!(
        word.to_ascii_lowercase().as_str(),
        "api"
            | "apis"
            | "method"
            | "methods"
            | "function"
            | "functions"
            | "member"
            | "members"
            | "field"
            | "fields"
            | "property"
            | "properties"
    )
}

fn comparison_or_coordination(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
    matches!(
        word.to_ascii_lowercase().as_str(),
        "and"
            | "or"
            | "vs"
            | "versus"
            | "compare"
            | "compares"
            | "compared"
            | "comparing"
            | "comparison"
            | "between"
            | "difference"
            | "differences"
            | "different"
            | "differs"
            | "similar"
            | "similarity"
            | "unlike"
    )
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

    // Frozen neutral contract using the real DSL and code-aware tokenizer.
    #[test]
    fn neutral_owner_role_bonus_contract() {
        let spec: serde_json::Value = serde_json::from_str(include_str!(
            "../../../artifacts/controls/query-context-owner-20261004/neutral-matrix.json"
        ))
        .unwrap();
        for row in spec["rows"].as_array().unwrap() {
            let raw = row["query"].as_str().unwrap();
            let dsl = crate::dsl::parse_search_dsl(raw);
            let target = QueryTarget::parse(&dsl);
            let actual = match &target {
                QueryTarget::Ambiguous => "fallback".into(),
                QueryTarget::ContainerMethods => "no-name-boost".into(),
                QueryTarget::Named { name, kind } => {
                    format!("named:{name}:{}", kind.map_or("any", |k| k.as_str()))
                }
            };
            assert_eq!(
                actual,
                row["expected_target_model"].as_str().unwrap(),
                "target: {raw}"
            );
            let context = NameBonusContext::parse(&dsl, &target);
            assert_eq!(
                context.contextual_owner.as_deref(),
                row["expected_context_owner"].as_str(),
                "context: {raw}"
            );
            let tokens = cc_db::fts::tokenize_codeish(&dsl.text);
            for probe in row["probes"].as_array().unwrap() {
                let name = probe["name"].as_str().unwrap();
                let kind = probe["kind"].as_str();
                let permitted = target.permits(name, kind, &tokens) && context.permits(name, kind);
                assert_eq!(
                    permitted,
                    probe["expected_bonus"].as_bool().unwrap(),
                    "bonus: {raw} / {name} / {kind:?}"
                );
            }
        }
    }
}
