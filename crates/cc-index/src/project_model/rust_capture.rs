//! Cache compact Rust syntax by the scanner's content hash, never mtime alone.
use super::super::indexer::ScanDiffResult;
use cc_model::{module_inputs::RustSourceInput, project_model::FileCatalog, CcResult};
use std::{collections::BTreeMap, path::Path};
pub(super) fn capture(
    root: &Path,
    files: &FileCatalog,
    old: &BTreeMap<String, RustSourceInput>,
    scan: Option<&ScanDiffResult>,
) -> CcResult<(BTreeMap<String, RustSourceInput>, usize, usize)> {
    let pending: BTreeMap<_, _> = scan
        .into_iter()
        .flat_map(|s| s.to_parse.iter())
        .map(|p| (p.scanned.rel_path.as_str(), p))
        .collect();
    let (mut out, mut reads, mut hits) = (BTreeMap::new(), 0, 0);
    for file in files.files().iter().filter(|p| p.ends_with(".rs")) {
        let expected = pending
            .get(file.as_str())
            .map(|p| p.content_hash.as_str())
            .or_else(|| {
                scan.and_then(|s| s.existing.get(file))
                    .map(|s| s.content_hash.as_str())
            });
        if let Some(cached) = old
            .get(file)
            .filter(|v| expected == Some(v.digest.as_str()))
        {
            out.insert(file.clone(), cached.clone());
            hits += 1;
            continue;
        }
        let (content, read) = super::source_capture::content(
            root,
            file,
            pending
                .get(file.as_str())
                .and_then(|p| p.content.as_deref()),
            expected,
        )?;
        reads += usize::from(read);
        let digest = blake3::hash(content.as_bytes()).to_hex().to_string();
        let facts = cc_parsers::rust_modules::extract(&content)?;
        out.insert(file.clone(), RustSourceInput { digest, facts });
    }
    Ok((out, reads, hits))
}
