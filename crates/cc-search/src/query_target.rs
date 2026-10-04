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
        if words.iter().any(|word| comparison_or_coordination(word)) || comparison_phrase(text) {
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

// A phrase is two maximal source words in the same validated lexical context.
// Group closure can follow the pair or its entire clause; delimiters are never
// erased between words. This is a lexical abstention rule, not intent parsing.
fn comparison_phrase(text: &str) -> bool {
    struct Group {
        start: usize,
        end: Option<usize>,
        closing: char,
        code: bool,
    }
    struct Word {
        start: usize,
        end: usize,
        contexts: Vec<usize>,
    }
    fn word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }
    fn opener(c: char) -> Option<char> {
        match c {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '"' => Some('"'),
            '\'' => Some('\''),
            '`' => Some('`'),
            '“' => Some('”'),
            '‘' => Some('’'),
            _ => None,
        }
    }
    let chars: Vec<_> = text.char_indices().collect();
    let mut groups: Vec<Group> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut words: Vec<Word> = Vec::new();
    let mut closed = std::collections::HashSet::new();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        if c == '\\' {
            // Escaped source characters cannot become grouping boundaries.
            i += 2;
            continue;
        }
        if word_char(c) {
            let start = at;
            i += 1;
            while i < chars.len() && word_char(chars[i].1) {
                i += 1;
            }
            words.push(Word {
                start,
                end: chars.get(i).map_or(text.len(), |v| v.0),
                contexts: stack.clone(),
            });
            continue;
        }
        if matches!(c, '\'' | '’')
            && i > 0
            && word_char(chars[i - 1].1)
            && chars.get(i + 1).is_some_and(|(_, next)| word_char(*next))
        {
            i += 1;
            continue;
        }
        if let Some(&id) = stack.last() {
            if groups[id].closing == c {
                groups[id].end = Some(at);
                groups[id].code |= chars.get(i + 1).is_some_and(|(_, next)| {
                    word_char(*next)
                        || matches!(next, '(' | '[' | '{' | '$' | '@' | '#' | '\\')
                        || (*next == '.'
                            && chars.get(i + 2).is_some_and(|(_, tail)| word_char(*tail)))
                });
                closed.insert(at);
                stack.pop();
                i += 1;
                continue;
            }
        }
        if matches!(c, ')' | ']' | '}' | '”' | '’') {
            return false;
        }
        // Apostrophes inside/after ordinary words are not opening quotes.
        if c == '\'' && i > 0 && word_char(chars[i - 1].1) {
            i += 1;
            continue;
        }
        if let Some(closing) = opener(c) {
            let code = i > 0
                && (word_char(chars[i - 1].1)
                    || matches!(chars[i - 1].1, '$' | '@' | '#' | '.' | ':' | '\\')
                    || closed.contains(&chars[i - 1].0));
            let id = groups.len();
            groups.push(Group {
                start: at,
                end: None,
                closing,
                code,
            });
            stack.push(id);
        }
        i += 1;
    }
    if !stack.is_empty() {
        return false;
    }
    let plain = |word: &Word| {
        if word.contexts.iter().any(|&id| groups[id].code) {
            return false;
        }
        let before = text[..word.start].char_indices().next_back();
        let before_ok = before.is_none_or(|(at, c)| {
            c.is_whitespace() || c == ',' || word.contexts.iter().any(|&id| groups[id].start == at)
        });
        let after = text[word.end..].chars().next();
        let after_ok = after.is_none_or(|c| {
            c.is_whitespace()
                || matches!(c, ',' | '?' | '!')
                || (c == '.' && !text[word.end..].chars().nth(1).is_some_and(word_char))
                || word
                    .contexts
                    .iter()
                    .any(|&id| groups[id].end == Some(word.end))
        });
        before_ok && after_ok
    };
    words.windows(2).any(|pair| {
        let left = &pair[0];
        let right = &pair[1];
        let a = &text[left.start..left.end];
        let b = &text[right.start..right.end];
        if !((a.eq_ignore_ascii_case("rather") && b.eq_ignore_ascii_case("than"))
            || (a.eq_ignore_ascii_case("instead") && b.eq_ignore_ascii_case("of")))
        {
            return false;
        }
        let gap = &text[left.end..right.start];
        let whitespace = gap.strip_prefix(',').unwrap_or(gap);
        left.contexts == right.contexts
            && plain(left)
            && plain(right)
            && !whitespace.is_empty()
            && whitespace.chars().all(char::is_whitespace)
    })
}

fn comparison_word(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
}

fn comparison_or_coordination(word: &str) -> bool {
    let word = comparison_word(word);
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

    #[test]
    fn query_target_context_comparison_phrases_retain_fallback() {
        for query in [
            "Which Beacon API rather than Sink accepts state?",
            "Which Beacon API RATHER THAN Sink accepts state?",
            "Which Beacon API rather\tthan Sink accepts state?",
            "Which Beacon API rather\nthan Sink accepts state?",
            "Which Beacon API (rather than) Sink accepts state?",
            "Which Beacon API rather, than Sink accepts state?",
            "Which API on Beacon rather than Sink accepts state?",
            "Which Beacon API instead of Sink accepts state?",
            "What method of Beacon INSTEAD OF Sink accepts state?",
        ] {
            let dsl = crate::dsl::parse_search_dsl(query);
            let target = QueryTarget::parse(&dsl);
            assert_eq!(target, QueryTarget::Ambiguous, "{query}");
            let context = NameBonusContext::parse(&dsl, &target);
            assert_eq!(context, NameBonusContext::default(), "{query}");
            let tokens = cc_db::fts::tokenize_codeish(&dsl.text);
            for kind in [
                "class",
                "interface",
                "type_alias",
                "enum",
                "module",
                "namespace",
            ] {
                assert!(target.permits("Beacon", Some(kind), &tokens));
                assert!(context.permits("Beacon", Some(kind)), "{query} / {kind}");
            }
            assert!(context.permits("Sink", Some("interface")));
        }
    }

    #[test]
    fn query_target_context_phrase_boundaries_and_independent_controls() {
        // Do not use code-aware token splitting: identifiers and substrings
        // are not standalone comparative words, nor are nonadjacent words.
        for tail in [
            "rather_than Sink accepts state",
            "RatherThan Sink accepts state",
            "rather.than Sink accepts state",
            "prather than Sink accepts state",
            "rather thanksgiving accepts state",
            "rather more than Sink accepts state",
            "rather _than Sink accepts state",
            "instead_of Sink accepts state",
            "InsteadOf Sink accepts state",
            "instead often accepts state",
            "instead only of Sink accepts state",
        ] {
            let query = format!("Which Beacon API {tail}?");
            let dsl = crate::dsl::parse_search_dsl(&query);
            let context = NameBonusContext::parse(&dsl, &QueryTarget::parse(&dsl));
            assert_eq!(
                context.contextual_owner.as_deref(),
                Some("beacon"),
                "{query}"
            );
            assert!(!context.permits("Beacon", Some("class")), "{query}");
        }
        // Exact independent probes, including the existing permissive tails.
        for (query, owner) in [
            ("Which Beacon API is deprecated?", Some("beacon")),
            ("Which Beacon API rather than Sink accepts state?", None),
            ("Which Beacon API calls Commit/Accept?", Some("beacon")),
            ("Which Beacon API differs from Sink?", None),
            ("name:Beacon Which Beacon API calls Commit?", None),
            ("kind:class Which Beacon API calls Commit?", None),
            ("Which Beacon API calls Commit and Accept?", None),
            ("Which Beacon type stores the ready state?", None),
            ("Beacon.Commit", None),
        ] {
            let dsl = crate::dsl::parse_search_dsl(query);
            let context = NameBonusContext::parse(&dsl, &QueryTarget::parse(&dsl));
            assert_eq!(context.contextual_owner.as_deref(), owner, "{query}");
        }
    }

    #[test]
    fn query_target_comparison_source_boundaries_keep_code_references() {
        for query in [
            "Which Beacon API calls `rather()` `than()` with Commit?",
            "Which Beacon API calls $rather $than with Commit?",
            "Which Beacon API calls rather() than() with Commit?",
            "Which Beacon API calls `rather` `than` with Commit?",
            "Which Beacon API calls \"rather\" \"than\" with Commit?",
            "Which Beacon API calls 'rather' 'than' with Commit?",
            "Which Beacon API calls (rather) (than) with Commit?",
            "Which Beacon API calls rather: than: with Commit?",
            "Which Beacon API calls rather! than? with Commit?",
            "Which Beacon API calls $instead $of with Commit?",
            "Which Beacon API calls `instead()` `of()` with Commit?",
            "Which Beacon API calls (rather than] with Commit?",
        ] {
            let dsl = crate::dsl::parse_search_dsl(query);
            let target = QueryTarget::parse(&dsl);
            assert_eq!(target, QueryTarget::Ambiguous, "{query}");
            let context = NameBonusContext::parse(&dsl, &target);
            assert_eq!(
                context.contextual_owner.as_deref(),
                Some("beacon"),
                "{query}"
            );
            assert!(!context.permits("Beacon", Some("class")), "{query}");
        }
        for phrase in [
            "rather than",
            "RATHER THAN",
            "rather\tthan",
            "rather\nthan",
            "rather\u{2003}than",
            "(rather than)",
            "rather, than",
            "\"rather than\"",
            "`rather than`",
            "instead of",
            "INSTEAD OF",
            "instead\tof",
            "(instead of)",
            "instead, of",
            "“instead of”",
        ] {
            let query = format!("Which Beacon API {phrase} Sink accepts state?");
            let dsl = crate::dsl::parse_search_dsl(&query);
            let context = NameBonusContext::parse(&dsl, &QueryTarget::parse(&dsl));
            assert_eq!(context, NameBonusContext::default(), "{query}");
        }
    }

    fn lexical_matrix_family(families: &[&str]) {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../artifacts/reviews/query-owner-lexical-spans-correction-20261004/lexical-matrix.json"
        )).unwrap();
        for row in rows.as_array().unwrap() {
            if !families.contains(&row["family"].as_str().unwrap()) {
                continue;
            }
            let raw = row["query"].as_str().unwrap();
            let dsl = crate::dsl::parse_search_dsl(raw);
            let target = QueryTarget::parse(&dsl);
            assert_eq!(target, QueryTarget::Ambiguous, "{raw}");
            let context = NameBonusContext::parse(&dsl, &target);
            let comparative = row["comparative"].as_bool().unwrap();
            assert_eq!(
                context.contextual_owner.as_deref(),
                if comparative { None } else { Some("beacon") },
                "{raw}"
            );
            for kind in [
                "class",
                "interface",
                "type_alias",
                "enum",
                "module",
                "namespace",
            ] {
                assert_eq!(
                    context.permits("Beacon", Some(kind)),
                    comparative,
                    "{raw} / {kind}"
                );
            }
            assert!(context.permits("Beacon", Some("method")));
            assert!(context.permits("Beacon", None));
            assert!(context.permits("Sink", Some("interface")));
        }
    }

    #[test]
    fn query_target_lexical_grouping_cross_product() {
        lexical_matrix_family(&["grouping_product"]);
    }

    #[test]
    fn query_target_lexical_code_decorations_cross_product() {
        lexical_matrix_family(&["decorations_product"]);
    }

    #[test]
    fn query_target_lexical_delimiters_and_independent_boundaries() {
        lexical_matrix_family(&["delimiters", "independent"]);
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
