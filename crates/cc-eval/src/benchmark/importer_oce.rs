//! Import externally supplied data, without vendoring the upstream corpus.
use super::{invalid, schema::*, validation, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
pub const REFERENCE_COMMIT: &str = "d4f10554a18e31599d1e46d5d56da6588d4aa86c";
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub name: String,
    pub remote: String,
    pub commit: String,
    pub describe: Option<String>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub schema_version: u32,
    pub benchmark: String,
    pub questions: usize,
    pub repository: Repository,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OceQuery {
    #[serde(alias = "query_id")]
    id: String,
    category: String,
    difficulty: u8,
    query: String,
    expected_files: Vec<String>,
    #[serde(default)]
    must_contain: Vec<String>,
}
pub fn import(data: &Path, metadata: &Path) -> Result<(Vec<Query>, Metadata)> {
    let raw = std::fs::read_to_string(data)?;
    let meta: Metadata = super::manifest::json_file(metadata)?;
    if meta.schema_version != 1
        || meta.repository.commit.len() != 40
        || !meta
            .repository
            .commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid("external metadata version/commit"));
    }
    if data.file_name().and_then(|s| s.to_str()) != Some(meta.benchmark.as_str()) {
        return Err(invalid("metadata filename mismatch"));
    }
    let mut rows = Vec::new();
    for line in raw
        .trim_start_matches('\u{feff}')
        .lines()
        .filter(|l| !l.trim().is_empty())
    {
        let o: OceQuery = serde_json::from_str(line)?;
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "must_contain_annotation_only".into(),
            serde_json::json!(o.must_contain),
        );
        annotations.insert(
            "upstream_reference".into(),
            serde_json::json!(REFERENCE_COMMIT),
        );
        rows.push(Query {
            id: o.id.clone(),
            category: o.category,
            difficulty: o.difficulty,
            language: "unspecified".into(),
            split: "dev".into(),
            query_family: format!("{}:{}", meta.repository.name, o.id),
            query: o.query,
            path_prefix: None,
            no_answer: false,
            expected_files: o
                .expected_files
                .into_iter()
                .map(|p| p.replace('\\', "/"))
                .collect(),
            answers: vec![],
            annotations,
        });
    }
    if rows.len() != meta.questions {
        return Err(invalid(format!(
            "declared {}, actual {} questions",
            meta.questions,
            rows.len()
        )));
    }
    validation::queries(&rows, ScoreProfile::OceCompat)?;
    Ok((rows, meta))
}
