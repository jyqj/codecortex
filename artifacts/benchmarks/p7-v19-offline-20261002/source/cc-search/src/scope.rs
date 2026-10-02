//! Materialize caller and DSL constraints once; ranking hints cannot enter this path.
use crate::dsl::ParsedQuery;
use cc_model::{retrieval::HardScope, search::SearchRequest, CcError, CcResult, Language};

pub(crate) fn normalize_request(
    request: &mut SearchRequest,
    dsl: &ParsedQuery,
) -> CcResult<HardScope> {
    let mut scope = HardScope::from(&*request);
    scope.path_prefix = scope
        .path_prefix
        .as_deref()
        .map(cc_model::repo_path::normalize_relative)
        .transpose()?;
    scope.file_paths = scope
        .file_paths
        .as_ref()
        .map(|files| {
            files
                .iter()
                .map(|p| {
                    let path = cc_model::repo_path::normalize_relative(p)?;
                    if path.is_empty() {
                        return Err(CcError::InvalidParams(
                            "file_paths requires a file path".into(),
                        ));
                    }
                    Ok(path)
                })
                .collect::<CcResult<Vec<_>>>()
        })
        .transpose()?;
    for files in [
        &mut request.boost_file_paths,
        &mut request.recent_file_paths,
        &mut request.pinned_file_paths,
        &mut request.overlay_file_paths,
    ]
    .into_iter()
    .flatten()
    {
        *files = files
            .iter()
            .filter_map(|p| cc_model::repo_path::normalize_relative(p).ok())
            .filter(|p| !p.is_empty())
            .collect();
    }
    for prefix in &dsl.path_filters {
        if prefix.is_empty() {
            return Err(CcError::InvalidParams(
                "path: requires a nonempty prefix".into(),
            ));
        }
        scope = scope.intersect(&HardScope {
            path_prefix: Some(cc_model::repo_path::normalize_relative(prefix)?),
            ..Default::default()
        });
    }
    for name in &dsl.lang_filters {
        let lang = Language::from_name(name);
        if lang == Language::Unknown && !name.eq_ignore_ascii_case("unknown") {
            return Err(CcError::InvalidParams(format!(
                "unknown or empty language filter: {name}"
            )));
        }
        scope = scope.intersect(&HardScope {
            languages: Some(vec![lang]),
            ..Default::default()
        });
    }
    // Materialize the conjunction even without DSL filters. This prunes an
    // explicit file set against its own prefix and makes impossible sets visible.
    scope = scope.intersect(&HardScope::default());
    request.path_prefix = scope.path_prefix.clone();
    request.languages = scope.languages.clone();
    request.file_paths = scope.file_paths.clone();
    // Do not run grep with a filter-only string or an empty regex. A name-only
    // query has an actual retrieval term; a pure scope query has no text hits.
    request.query = if dsl.text.is_empty() {
        dsl.name_filter.clone().unwrap_or_default()
    } else {
        dsl.text.clone()
    };
    Ok(scope)
}
pub(crate) fn chunk_scope(scope: &HardScope) -> cc_db::ChunkScope {
    cc_db::ChunkScope {
        path_prefix: scope.path_prefix.clone(),
        languages: scope
            .languages
            .as_ref()
            .map(|ls| ls.iter().map(|l| l.as_str().to_string()).collect()),
        file_paths: scope.file_paths.clone(),
    }
}
