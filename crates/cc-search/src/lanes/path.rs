use std::collections::{HashMap, HashSet};

use cc_model::config::SearchConfig;
use cc_model::retrieval::{LaneCoverage, LaneStatus};
use cc_model::CcResult;

use super::{LaneContext, LaneRun, RetrievalLane, LANE_PATH};

pub(crate) struct PathLane;

// A whole canonical multi-component path is a different retrieval domain
// from prose/component hints. Spaces/aliases keep the old fallback behavior.
fn canonical_path_domain(query: &str) -> bool {
    !query.chars().any(char::is_whitespace)
        && !query.contains('\\')
        && query.split('/').count() >= 2
        && cc_model::repo_path::normalize_relative(query).is_ok_and(|path| path == query)
}

impl RetrievalLane for PathLane {
    fn lane_id(&self) -> &'static str {
        LANE_PATH
    }

    fn weight(&self, config: &SearchConfig) -> f64 {
        config.path_weight
    }

    fn is_enabled(&self, context: &LaneContext<'_>) -> bool {
        context.config.path_weight > 0.0
            && !context.plan.is_empty_scope()
            && !context.plan.primary_query_text().is_empty()
    }

    fn annotates_hits(&self) -> bool {
        true
    }

    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        self.run_detailed(context).map(|run| run.hits)
    }

    fn run_detailed(&self, context: &LaneContext<'_>) -> CcResult<LaneRun> {
        let limit = context.config.path_top_k.max(context.plan.limits().top_k);
        let scope = context.plan.chunk_scope();
        let mut exact = context.db.retrieval().exact_path_chunk_hits(
            context.plan.primary_query_text(),
            &scope,
            limit.saturating_add(1),
        )?;
        let exact_ids: HashSet<String> = exact.iter().map(|(id, _)| id.clone()).collect();

        // This is scoped indexed-document identity, not disk freshness or
        // whole-file body completeness. Hydration still verifies current
        // source/version. No generic token scan is executed in this domain.
        if !exact.is_empty() && canonical_path_domain(context.plan.primary_query_text()) {
            let lower_bound = exact.len();
            let truncated = lower_bound > limit;
            exact.truncate(limit);
            return Ok(LaneRun {
                status: if truncated {
                    LaneStatus::Partial
                } else {
                    LaneStatus::Complete
                },
                coverage: if truncated {
                    LaneCoverage::partial(None, lower_bound)
                } else {
                    LaneCoverage::complete(None, exact.len())
                },
                truncation_reason: truncated.then(|| "candidate_limit".into()),
                hits: exact,
                exact_ids,
                grep: None,
                lexical_work: Default::default(),
            });
        }

        // Multi-token prose often contains incidental one/two-character
        // words (a, is, ...). Those are not useful substring path evidence.
        // Keep short tokens for a standalone query and exact paths above.
        // Apply this eligibility rule before the token work budget, so ignored
        // prose is not falsely reported as truncated retrieval.
        let primary = context.plan.primary_query_tokens();
        let mut eligible = primary.iter().filter(|token| {
            !token.is_empty() && (primary.len() == 1 || token.chars().count() >= 3)
        });
        let tokens: Vec<&str> = eligible.by_ref().take(8).map(String::as_str).collect();
        let tokens_omitted = eligible.next().is_some();
        let file_limit = limit.saturating_mul(4).max(32);
        let token_files = context.db.retrieval().path_token_file_hits_many_scoped(
            &tokens,
            &scope,
            file_limit.saturating_add(1),
        )?;
        let token_truncated =
            token_files.iter().any(|files| files.len() > file_limit) || tokens_omitted;
        let mut file_scores: HashMap<String, usize> = HashMap::new();
        for files in token_files {
            for file in files.into_iter().take(file_limit) {
                *file_scores.entry(file).or_default() += 1;
            }
        }
        let mut files: Vec<(String, usize)> = file_scores.into_iter().collect();
        files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let file_refs: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
        let first_chunks = context
            .db
            .retrieval()
            .first_chunk_ids_for_files(&file_refs)?;

        let mut seen: HashSet<String> = exact.iter().map(|(id, _)| id.clone()).collect();
        let token_count = tokens.len().max(1) as f64;
        for (file, matches) in files {
            let Some(chunk_id) = first_chunks.get(&file) else {
                continue;
            };
            if seen.insert(chunk_id.clone()) {
                exact.push((chunk_id.clone(), matches as f64 / token_count));
            }
        }
        let lower_bound = exact.len();
        let truncated = exact.len() > limit || token_truncated;
        exact.truncate(limit);
        Ok(LaneRun {
            status: if truncated {
                LaneStatus::Partial
            } else {
                LaneStatus::Complete
            },
            coverage: if truncated {
                LaneCoverage::partial(None, lower_bound)
            } else {
                LaneCoverage::complete(None, exact.len())
            },
            truncation_reason: truncated.then(|| {
                if token_truncated {
                    "path_token_limit"
                } else {
                    "candidate_limit"
                }
                .into()
            }),
            hits: exact,
            exact_ids,
            grep: None,
            lexical_work: Default::default(),
        })
    }
}

#[cfg(test)]
mod domain_tests {
    use super::canonical_path_domain;
    #[test]
    fn shortcut_requires_whole_canonical_multicomponent_path() {
        for q in ["src/a.rs", "文档/Guide.md", "src/deep/x.py"] {
            assert!(canonical_path_domain(q), "{q}");
        }
        for q in [
            "a.rs",
            "needle",
            "./src/a.rs",
            "src//a.rs",
            "../src/a.rs",
            "/src/a.rs",
            "src\\a.rs",
            "fix src/a.rs",
            "src/foo bar.rs",
            "src/\u{2003}x.rs",
            "src/",
            "",
        ] {
            assert!(!canonical_path_domain(q), "{q}");
        }
    }
}
