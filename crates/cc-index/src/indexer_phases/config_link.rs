use std::collections::{HashMap, HashSet};
use std::path::Path;

use cc_db::index_db::{FileWriteUnit, SymbolTargetRow};
use cc_model::edge::ResolutionKind;
use cc_model::parse::ParseOutcome;
use cc_model::symbol::SymbolRefRecord;
use cc_model::{BuildExplainCollector, CcResult, Language, ParserTier, StableId};

use crate::config_linker::{
    config_files_signature, resolve_config_links, scan_config_tokens, ConfigLinkKind,
    RawConfigToken,
};
use crate::indexer::Indexer;

use super::{time_step, CONFIG_SIG_ALGORITHM};

/// Metadata keys for the config-linker gate: the config-file-set signature
/// (paths + mtime + size, mirroring `last_infra_sig`), its algorithm version,
/// and the cached raw token extraction the signature validates.
pub(super) const CONFIG_SIG_KEY: &str = "last_config_sig";
pub(super) const CONFIG_SIG_ALGO_KEY: &str = "last_config_sig_algo";
pub(super) const CONFIG_RAW_CACHE_KEY: &str = "config_raw_tokens";

/// Upper bound for the persisted raw-token cache. Projects whose config scan
/// produces more serialized tokens than this (huge lock files) simply skip
/// the cache and rescan each build — the pre-gate behavior.
const CONFIG_RAW_CACHE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// One non-skipped round of the incremental config-link pass: the units to
/// write plus the config files the scan (or token cache) covered. A seen
/// file without a unit resolved to zero links this round — apply uses the
/// list to clear such files' stale refs from earlier rounds.
pub(super) struct ConfigLinkRound {
    pub(super) units: Vec<FileWriteUnit>,
    pub(super) seen_config_files: Vec<String>,
}

impl Indexer {
    /// Pure function: build config link units from pre-collected snapshot data
    /// plus pre-scanned raw config tokens (see [`scan_config_tokens`]).
    /// Does not query the database, suitable for use inside temp-db write closure.
    pub(super) fn build_config_link_units_from_snapshot(
        project_path: &Path,
        symbol_targets: Vec<SymbolTargetRow>,
        indexed_files: &[String],
        raw_tokens: &[RawConfigToken],
    ) -> CcResult<Vec<FileWriteUnit>> {
        let mut known_symbols = HashSet::new();
        let mut qname_lookup: HashMap<String, (String, Option<String>, String)> = HashMap::new();
        let mut basename_lookup: HashMap<String, Vec<(String, Option<String>, String)>> =
            HashMap::new();
        for sym in symbol_targets {
            if let Some(qname) = sym.qname.clone() {
                known_symbols.insert(qname.clone());
                qname_lookup.insert(
                    qname,
                    (
                        sym.symbol_id.clone(),
                        sym.symbol_uid.clone(),
                        sym.file_path.clone(),
                    ),
                );
            }
            basename_lookup.entry(sym.name.clone()).or_default().push((
                sym.symbol_id,
                sym.symbol_uid,
                sym.file_path,
            ));
        }

        let known_files: HashSet<String> = indexed_files.iter().cloned().collect();
        let mut file_basename_lookup: HashMap<String, Vec<String>> = HashMap::new();
        for file in indexed_files {
            if let Some(base) = Path::new(file).file_name().and_then(|n| n.to_str()) {
                file_basename_lookup
                    .entry(base.to_string())
                    .or_default()
                    .push(file.clone());
            }
        }
        let links = resolve_config_links(raw_tokens, &known_symbols, &known_files);
        if links.is_empty() {
            return Ok(Vec::new());
        }

        let mut grouped: HashMap<String, Vec<_>> = HashMap::new();
        for link in links {
            grouped
                .entry(link.config_file.clone())
                .or_default()
                .push(link);
        }

        let mut units = Vec::new();
        for (config_file, links) in grouped {
            let abs_path = project_path.join(&config_file);
            let content = match std::fs::read_to_string(&abs_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let metadata = match abs_path.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mut symbol_refs = Vec::new();
            for link in &links {
                let (
                    target_symbol_id,
                    target_symbol_uid,
                    target_file_path,
                    resolution_kind,
                    resolution_confidence,
                    resolution_strategy,
                ) = match link.link_kind {
                    ConfigLinkKind::ModulePath => {
                        if let Some((sid, suid, fpath)) = qname_lookup.get(&link.referenced_value) {
                            (
                                Some(sid.clone()),
                                suid.clone(),
                                Some(fpath.clone()),
                                ResolutionKind::Exact,
                                link.confidence,
                                "config_module_exact".to_string(),
                            )
                        } else {
                            let tail = link
                                .referenced_value
                                .rsplit('.')
                                .next()
                                .unwrap_or(&link.referenced_value);
                            match basename_lookup.get(tail) {
                                Some(candidates) if candidates.len() == 1 => {
                                    let (sid, suid, fpath) = &candidates[0];
                                    (
                                        Some(sid.clone()),
                                        suid.clone(),
                                        Some(fpath.clone()),
                                        ResolutionKind::Heuristic,
                                        link.confidence,
                                        "config_module_suffix".to_string(),
                                    )
                                }
                                _ => (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                ),
                            }
                        }
                    }
                    ConfigLinkKind::FilePath => {
                        let resolved_path = if known_files.contains(&link.referenced_value) {
                            Some(link.referenced_value.clone())
                        } else {
                            Path::new(&link.referenced_value)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .and_then(|base| file_basename_lookup.get(base))
                                .filter(|paths| paths.len() == 1)
                                .and_then(|paths| paths.first().cloned())
                        };
                        match resolved_path {
                            Some(path) => (
                                None,
                                None,
                                Some(path),
                                if known_files.contains(&link.referenced_value) {
                                    ResolutionKind::Exact
                                } else {
                                    ResolutionKind::Heuristic
                                },
                                link.confidence,
                                if known_files.contains(&link.referenced_value) {
                                    "config_file_exact".to_string()
                                } else {
                                    "config_file_basename".to_string()
                                },
                            ),
                            None => (
                                None,
                                None,
                                None,
                                ResolutionKind::Unresolved,
                                0.0,
                                "unresolved".to_string(),
                            ),
                        }
                    }
                    ConfigLinkKind::DependencyImport => {
                        if let Some((sid, suid, fpath)) = qname_lookup.get(&link.referenced_value) {
                            (
                                Some(sid.clone()),
                                suid.clone(),
                                Some(fpath.clone()),
                                ResolutionKind::Exact,
                                link.confidence,
                                "config_dependency_exact".to_string(),
                            )
                        } else if let Some(candidates) = basename_lookup.get(&link.referenced_value)
                        {
                            if candidates.len() == 1 {
                                let (sid, suid, fpath) = &candidates[0];
                                (
                                    Some(sid.clone()),
                                    suid.clone(),
                                    Some(fpath.clone()),
                                    ResolutionKind::Heuristic,
                                    link.confidence,
                                    "config_dependency_symbol".to_string(),
                                )
                            } else {
                                (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                )
                            }
                        } else if let Some(paths) = file_basename_lookup.get(&link.referenced_value)
                        {
                            if paths.len() == 1 {
                                (
                                    None,
                                    None,
                                    Some(paths[0].clone()),
                                    ResolutionKind::Heuristic,
                                    link.confidence,
                                    "config_dependency_file".to_string(),
                                )
                            } else {
                                (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                )
                            }
                        } else {
                            (
                                None,
                                None,
                                None,
                                ResolutionKind::Unresolved,
                                0.0,
                                "unresolved".to_string(),
                            )
                        }
                    }
                };

                symbol_refs.push(SymbolRefRecord {
                    ref_id: StableId::ref_id(&config_file, &link.referenced_value, link.line, 0),
                    file_path: config_file.clone(),
                    symbol_name: link.referenced_value.clone(),
                    container: Some(link.config_key.clone()),
                    ref_kind: match link.link_kind {
                        ConfigLinkKind::ModulePath => "config_module".to_string(),
                        ConfigLinkKind::FilePath => "config_file".to_string(),
                        ConfigLinkKind::DependencyImport => "config_dependency".to_string(),
                    },
                    line: link.line,
                    column: 0,
                    target_symbol_id,
                    target_file_path,
                    target_symbol_uid,
                    ref_name: Some(link.referenced_value.clone()),
                    scope_id: None,
                    resolution_kind,
                    resolution_confidence,
                    resolution_strategy,
                    ref_end_line: Some(link.line),
                    ref_end_col: None,
                    parser_tier: ParserTier::Heuristic,
                    parser_confidence: link.confidence.max(0.70),
                });
            }

            let excerpt: String = links
                .iter()
                .take(6)
                .map(|link| format!("{} -> {}", link.config_key, link.referenced_value))
                .collect::<Vec<_>>()
                .join("; ");

            let outcome = ParseOutcome {
                summary: format!(
                    "Configuration file with {} code link(s){}",
                    symbol_refs.len(),
                    if excerpt.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", excerpt)
                    }
                ),
                symbol_refs,
                parser_tier: ParserTier::Heuristic,
                parser_confidence: 0.85,
                ..Default::default()
            };

            let content_hash = crate::indexer::content_hash_hex(content.as_bytes());
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            units.push(FileWriteUnit {
                rel_path: config_file,
                language: Language::Unknown,
                content_hash,
                mtime,
                size: metadata.len(),
                outcome,
            });
        }

        Ok(units)
    }

    /// Incremental config-link pass behind a file-set signature gate.
    ///
    /// The expensive half (project walk + read + tokenize, see
    /// [`scan_config_tokens`]) only depends on the config files themselves, so
    /// it is skipped when the config signature (paths + mtime + size) is
    /// unchanged — the raw tokens are then served from the metadata cache.
    /// The cheap half (resolving tokens against the symbol/file catalog) must
    /// still run whenever this build wrote index content, because links
    /// appear/disappear with the catalog (a removed symbol must not keep its
    /// link). When the signature is unchanged AND the batch wrote nothing,
    /// the catalog provably did not change either and the whole pass —
    /// including the catalog reads — is skipped (`None`).
    ///
    /// Second skip (zero-token fast path): when the signature is unchanged
    /// and the cached raw tokens deserialize to an EMPTY list, the resolve
    /// half is a provable no-op — zero tokens resolve to zero links AND an
    /// empty seen-file list, so `apply_config_link_units` would write
    /// nothing — and the catalog reads are skipped even for non-empty
    /// batches. This cannot swallow stale-row clearing: a previous round
    /// that produced links did so from non-empty tokens, and the cache is
    /// recorded together with the signature, so an unchanged signature
    /// serves those same non-empty tokens and the fast path does not fire.
    ///
    /// `Some` means the resolve half ran; the caller must hand the round to
    /// `apply_config_link_units` even when `units` is empty, so that files
    /// which resolved to zero links this round get their stale refs cleared.
    ///
    /// Not a `PassGate` adapter, deliberately. The postprocess/analysis gates
    /// are binary run/skip with deferred signature records (the compute→apply
    /// seam in `pass_gate.rs`). This is a WRITE-stage gate with a three-way
    /// outcome (skip-entirely / reuse-cached-tokens / rescan), an *immediate*
    /// signature record (`scan_and_record_config_tokens` writes through the
    /// write facet in the same step — there is no deferred-record window), and
    /// an extra raw-token cache dimension. It shares the algo-versioned
    /// u64-signature compare *pattern* with `FileSignatureGate` but does not
    /// fit the deferred-record shape, so forcing it into `PassGate` would
    /// either distort the seam or encode the three-way outcome in a reason
    /// string. The signature-compare pattern, not the trait, is what unifies
    /// them.
    pub(super) fn build_config_link_units_gated(
        &self,
        project_path: &Path,
        batch_empty: bool,
        walk_manifest: Option<&crate::scanner::WalkManifest>,
        scope_hints: Option<&crate::indexer::ScopeSignatureHints>,
        build_explain: &mut BuildExplainCollector,
    ) -> CcResult<Option<ConfigLinkRound>> {
        let recorded_algo = self
            .db
            .reads()
            .get_metadata(CONFIG_SIG_ALGO_KEY)?
            .unwrap_or_else(|| "1".to_string());
        let recorded_sig = self
            .db
            .reads()
            .get_metadata(CONFIG_SIG_KEY)?
            .and_then(|s| s.parse::<u64>().ok());

        // Event-scoped fast path: a walk-free build whose event set provably
        // contains no config-linker candidate cannot have changed the config
        // signature — reuse the recorded one instead of running the fallback
        // walk. Requires a comparable record (matching algo + present sig);
        // otherwise fall through to the compute so the gate behaves exactly
        // like the unscoped path on first build / algo upgrades.
        let scoped_reuse = walk_manifest.is_none()
            && scope_hints.is_some_and(|h| h.config_files_unaffected)
            && recorded_algo == CONFIG_SIG_ALGORITHM
            && recorded_sig.is_some();

        let (sig, unchanged) = if scoped_reuse {
            tracing::debug!(
                "config linker: scoped build touched no config candidates, reusing recorded signature"
            );
            (recorded_sig.unwrap_or_default(), true)
        } else {
            let sig = time_step("write", "config_sig_walk", || match walk_manifest {
                // Shared-walk manifest: signature without another tree walk
                // (value-equal to the walk fallback for the same tree).
                Some(manifest) => {
                    crate::config_linker::config_files_signature_from_manifest(manifest)
                }
                None => config_files_signature(project_path),
            });
            let unchanged =
                recorded_algo == CONFIG_SIG_ALGORITHM && recorded_sig == Some(sig);
            (sig, unchanged)
        };

        if unchanged && batch_empty {
            build_explain.record_gate("config_link", false, "signature unchanged and batch empty");
            tracing::debug!("config linker: signature unchanged and batch empty, skipping");
            return Ok(None);
        }

        let raw_tokens = if unchanged {
            // 签名未变：原始 token 与上次一致，优先用缓存，缓存缺失/损坏则重扫。
            match time_step("write", "config_token_cache", || {
                self.db
                    .reads()
                    .get_metadata(CONFIG_RAW_CACHE_KEY)
                    .map(|cached| {
                        cached.and_then(|json| {
                            serde_json::from_str::<Vec<RawConfigToken>>(&json).ok()
                        })
                    })
            })? {
                Some(tokens) if tokens.is_empty() => {
                    // 零 token 快路径：零 token ⇒ 零链接且 seen 为空 ⇒ apply
                    // 必为 no-op，连 catalog 读取一起跳过。上轮若有链接，其
                    // token 非空且与签名一同落盘，签名未变时缓存命中的就是
                    // 那批非空 token —— 不会走到这里，陈旧行清理不受影响。
                    build_explain.record_gate(
                        "config_link",
                        false,
                        "signature unchanged, cached tokens empty",
                    );
                    tracing::debug!(
                        "config linker: signature unchanged and cached tokens empty, skipping"
                    );
                    return Ok(None);
                }
                Some(tokens) => {
                    build_explain.record_gate(
                        "config_link",
                        true,
                        "signature unchanged, reused cached tokens",
                    );
                    tracing::debug!(
                        tokens = tokens.len(),
                        "config linker: scan skipped, resolving cached raw tokens"
                    );
                    tokens
                }
                None => {
                    build_explain.record_gate(
                        "config_link",
                        true,
                        "signature unchanged but token cache missing, rescanned",
                    );
                    self.scan_and_record_config_tokens(project_path, walk_manifest, sig)?
                }
            }
        } else {
            build_explain.record_gate("config_link", true, "signature changed, rescanned");
            self.scan_and_record_config_tokens(project_path, walk_manifest, sig)?
        };

        // 本轮扫描（或缓存）覆盖到的配置文件：没有产出单元的即为零链接，
        // apply 时按此清理它们的陈旧 refs。
        let mut seen_config_files: Vec<String> = raw_tokens
            .iter()
            .map(|token| token.config_file.clone())
            .collect();
        seen_config_files.sort();
        seen_config_files.dedup();

        let symbol_targets = time_step("write", "config_symbol_targets", || {
            self.db.reads().list_symbol_targets()
        })?;
        let indexed_files = time_step("write", "config_file_paths", || {
            self.db.reads().list_file_paths()
        })?;
        let units = time_step("write", "config_resolve", || {
            Self::build_config_link_units_from_snapshot(
                project_path,
                symbol_targets,
                &indexed_files,
                &raw_tokens,
            )
        })?;
        Ok(Some(ConfigLinkRound {
            units,
            seen_config_files,
        }))
    }

    /// Run the config scan and persist the gate state. The cache is written
    /// before the signature so a mid-write failure can only leave a stale/
    /// missing signature — which forces a rescan, never a wrong skip.
    fn scan_and_record_config_tokens(
        &self,
        project_path: &Path,
        walk_manifest: Option<&crate::scanner::WalkManifest>,
        sig: u64,
    ) -> CcResult<Vec<RawConfigToken>> {
        let raw_tokens = time_step("write", "config_token_scan", || match walk_manifest {
            Some(manifest) => crate::config_linker::scan_config_tokens_from_manifest(
                project_path,
                manifest,
            ),
            None => scan_config_tokens(project_path),
        })?;
        match Self::serialize_raw_token_cache(&raw_tokens) {
            // 超出缓存上限：清掉旧缓存，避免新签名配上陈旧 token。
            None => self.db.writes().set_metadata(CONFIG_RAW_CACHE_KEY, "")?,
            Some(serialized) => self
                .db
                .writes()
                .set_metadata(CONFIG_RAW_CACHE_KEY, &serialized)?,
        }
        self.db
            .writes()
            .set_metadata(CONFIG_SIG_KEY, &sig.to_string())?;
        self.db
            .writes()
            .set_metadata(CONFIG_SIG_ALGO_KEY, CONFIG_SIG_ALGORITHM)?;
        Ok(raw_tokens)
    }

    /// Serialize raw tokens for the metadata cache; `None` when over the cap.
    pub(super) fn serialize_raw_token_cache(raw_tokens: &[RawConfigToken]) -> Option<String> {
        let serialized = serde_json::to_string(raw_tokens).ok()?;
        if serialized.len() > CONFIG_RAW_CACHE_MAX_BYTES {
            tracing::debug!(
                bytes = serialized.len(),
                "config linker: raw token cache over cap, scan will rerun next build"
            );
            return None;
        }
        Some(serialized)
    }
}

#[cfg(test)]
mod config_linker_gate_tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    const INI_LIB: &str = "script = src/lib.py\n";
    /// Same byte length as [`INI_LIB`], so a swap keeps size (and, with the
    /// mtime restored, the config-file signature) unchanged.
    const INI_WIN: &str = "script = src/win.py\n";

    fn setup_project(ini_content: &str) -> (TempDir, Arc<IndexDb>, Indexer) {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/lib.py"),
            "def lib_handler():\n    return 1\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/win.py"),
            "def win_handler():\n    return 2\n",
        )
        .unwrap();
        std::fs::write(project.join("settings.ini"), ini_content).unwrap();
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        (tmp, db, indexer)
    }

    /// Resolved config-link targets recorded for `settings.ini`, line order.
    fn config_ref_targets(db: &IndexDb) -> Vec<String> {
        db.reads()
            .query_json(
                "SELECT target_file_path FROM symbol_refs \
                 WHERE file_path = 'settings.ini' AND target_file_path IS NOT NULL \
                 ORDER BY line",
                &[],
            )
            .unwrap()
            .iter()
            .filter_map(|row| {
                row.get("target_file_path")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect()
    }

    /// Rewrite a file in place, restoring its original mtime so the
    /// stat-based config signature cannot observe the change (same length
    /// content keeps the size component identical too).
    fn rewrite_preserving_mtime(path: &Path, content: &str) {
        let original_mtime = std::fs::metadata(path).unwrap().modified().unwrap();
        std::fs::write(path, content).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(original_mtime))
            .unwrap();
    }

    /// (a) When the config-file set signature is unchanged, an incremental
    /// build must skip the config scan and re-resolve the cached raw tokens:
    /// rewriting the config in a stat-invisible way must NOT be picked up
    /// (proving the file was never re-read), while the recorded gate
    /// metadata stays put.
    #[test]
    fn incremental_build_with_unchanged_signature_resolves_from_cache() {
        let (tmp, db, indexer) = setup_project(INI_LIB);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(config_ref_targets(&db), vec!["src/lib.py"]);
        let sig = db
            .reads()
            .get_metadata(CONFIG_SIG_KEY)
            .unwrap()
            .expect("config signature recorded");
        assert!(
            !db.reads()
                .get_metadata(CONFIG_RAW_CACHE_KEY)
                .unwrap()
                .expect("raw token cache recorded")
                .is_empty(),
            "raw token cache must be persisted"
        );

        // Stat-invisible rewrite + a source edit so the batch is non-empty
        // (re-resolution must still run against the current catalog).
        rewrite_preserving_mtime(&project.join("settings.ini"), INI_WIN);
        std::fs::write(
            project.join("src/lib.py"),
            "def lib_handler():\n    return 1\n\n\ndef lib_extra():\n    return 3\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/lib.py"],
            "scan skipped: links must reflect the cached tokens, not the rewritten file"
        );
        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap().as_deref(),
            Some(sig.as_str()),
            "unchanged signature must stay recorded"
        );
    }

    /// (b) A visibly modified config file (size change) must be rescanned
    /// and its links rebuilt from the new content.
    #[test]
    fn modified_config_file_is_rescanned_and_relinked() {
        let (tmp, db, indexer) = setup_project(INI_LIB);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(config_ref_targets(&db), vec!["src/lib.py"]);

        // Different size → signature changes regardless of mtime resolution.
        std::fs::write(
            project.join("settings.ini"),
            "script = src/win.py\nextra_flag = 1\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/win.py"],
            "changed config file must be re-scanned and re-linked"
        );
    }

    /// (c) Full builds always scan, even when the recorded signature matches
    /// the (stat-invisible) on-disk state.
    #[test]
    fn full_build_always_rescans_config_files() {
        let (tmp, db, indexer) = setup_project(INI_LIB);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(config_ref_targets(&db), vec!["src/lib.py"]);

        rewrite_preserving_mtime(&project.join("settings.ini"), INI_WIN);
        indexer.build_index(project, true).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/win.py"],
            "full build must rescan config files unconditionally"
        );
    }

    /// (d) Removing a file referenced by a config link must drop that link on
    /// the next incremental build even though the config scan is skipped —
    /// re-resolution of the cached raw tokens against the current catalog is
    /// what keeps links from dangling.
    #[test]
    fn removed_link_target_does_not_leave_dangling_config_link() {
        let (tmp, db, indexer) = setup_project("script = src/lib.py\nhelper = src/win.py\n");
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            config_ref_targets(&db),
            vec!["src/lib.py", "src/win.py"],
            "both links must resolve initially"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        std::fs::remove_file(project.join("src/win.py")).unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/lib.py"],
            "the link to the removed file must be gone"
        );
        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config files did not change: the scan must have been skipped"
        );
    }
}

#[cfg(test)]
mod config_link_write_path_tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// app.yaml 是 scanner 可见的配置文件（Language::Yaml）：同时走解析通道
    /// （generic chunker 产出 chunks）和 config-link 通道（file-path ref）。
    fn setup_yaml_project() -> (TempDir, Arc<IndexDb>, Indexer) {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/only.py"),
            "def only_handler():\n    return 1\n",
        )
        .unwrap();
        std::fs::write(project.join("app.yaml"), "script: src/only.py\n").unwrap();
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        (tmp, db, indexer)
    }

    fn count(db: &IndexDb, sql: &str) -> i64 {
        db.reads()
            .query_json(sql, &[])
            .unwrap()
            .first()
            .and_then(|row| row.get("cnt"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }

    /// app.yaml 上的 config-link refs 行数。
    fn config_ref_count(db: &IndexDb) -> i64 {
        count(
            db,
            "SELECT COUNT(*) AS cnt FROM symbol_refs WHERE file_path = 'app.yaml' \
             AND ref_kind IN ('config_module','config_file','config_dependency')",
        )
    }

    fn yaml_files_rows(db: &IndexDb) -> i64 {
        count(
            db,
            "SELECT COUNT(*) AS cnt FROM files WHERE file_path = 'app.yaml'",
        )
    }

    fn yaml_chunks(db: &IndexDb) -> i64 {
        count(
            db,
            "SELECT COUNT(*) AS cnt FROM chunks WHERE file_path = 'app.yaml'",
        )
    }

    /// 缺陷 A / 变体 (a)：配置文件集未变（签名不变 → cached-token 路径）。
    /// 删除被引用文件后本轮解析为零链接，不再产出替换单元 —— 旧 refs 必须
    /// 被显式清理，且 app.yaml 的 files 行保持存在且唯一。
    #[test]
    fn zero_link_resolution_clears_stale_refs_via_cached_tokens() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(
            config_ref_count(&db) > 0,
            "premise: initial build links app.yaml -> src/only.py"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        std::fs::remove_file(project.join("src/only.py")).unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config files unchanged: this run must take the cached-token path"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "zero-link resolution must clear the stale config refs"
        );
        assert_eq!(
            yaml_files_rows(&db),
            1,
            "app.yaml keeps exactly one files row"
        );
    }

    /// 缺陷 A / 变体 (b)：另一个配置文件被改动（签名变化 → fresh-scan 路径），
    /// 而 app.yaml 本身未变、不会被重新解析 —— 陈旧 refs 同样必须清理。
    #[test]
    fn zero_link_resolution_clears_stale_refs_via_fresh_scan() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(
            config_ref_count(&db) > 0,
            "premise: initial build links app.yaml -> src/only.py"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        std::fs::remove_file(project.join("src/only.py")).unwrap();
        // 触碰另一个配置文件改变配置集签名，强制 fresh scan。
        std::fs::write(project.join("settings.ini"), "flag = 1\n").unwrap();
        indexer.build_index(project, false).unwrap();

        assert_ne!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config set changed: this run must take the fresh-scan path"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "zero-link resolution must clear the stale config refs"
        );
        assert_eq!(
            yaml_files_rows(&db),
            1,
            "app.yaml keeps exactly one files row"
        );
    }

    /// 缺陷 B：full build 下 scanner 可见的 yaml 既在解析集又产出 config 单元，
    /// 必须恰好落库一次：解析产物（chunks、yaml language）与 config refs 共存；
    /// 同一棵树的增量构建收敛到同一状态。
    #[test]
    fn full_build_writes_linked_yaml_once_with_parsed_and_config_data() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer
            .build_index(project, true)
            .expect("full build over a linked yaml config must succeed");

        let snapshot = |db: &IndexDb| {
            (
                yaml_files_rows(db),
                yaml_chunks(db),
                config_ref_count(db),
                db.reads()
                    .query_json(
                        "SELECT language FROM files WHERE file_path = 'app.yaml'",
                        &[],
                    )
                    .unwrap()
                    .first()
                    .and_then(|row| row.get("language"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
            )
        };
        let full_state = snapshot(&db);
        assert_eq!(full_state.0, 1, "exactly one files row for app.yaml");
        assert!(full_state.1 > 0, "parsed chunks must be preserved");
        assert!(full_state.2 > 0, "config refs must be present");
        assert_eq!(
            full_state.3.as_deref(),
            Some("yaml"),
            "parsed language must be preserved"
        );

        // 同一 DB 上的增量重建必须收敛（不破坏已合并状态）。
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            snapshot(&db),
            full_state,
            "incremental rebuild on the same db must converge"
        );

        // 同一棵树、全新 DB 的纯增量构建也必须得到同一状态。
        let db2 = Arc::new(IndexDb::open(&project.join("index2.sqlite3")).unwrap().0);
        let indexer2 = Indexer::new(db2.clone(), project, &IndexingConfig::default());
        indexer2.build_index(project, false).unwrap();
        assert_eq!(
            snapshot(&db2),
            full_state,
            "fresh incremental build must match the full-build state"
        );
    }

    /// C4 边界：上轮有链接 → 本轮配置内容被清空（token 归零）。归零必然改变
    /// 配置签名，走 fresh-scan 路径清除旧链接（快路径条件 unchanged=false，
    /// 不可能触发）；其后签名稳定、缓存 token 为空的增量轮走零 token 快路径，
    /// 既不得遗留也不得复活任何 config refs。
    #[test]
    fn zero_token_fast_path_does_not_swallow_link_clearing() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(
            config_ref_count(&db) > 0,
            "premise: initial build links app.yaml -> src/only.py"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        // 清空链接内容：签名变化 → fresh scan → 零 token，旧链接必须被清除。
        std::fs::write(project.join("app.yaml"), "note: nothing here\n").unwrap();
        indexer.build_index(project, false).unwrap();
        assert_ne!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config content changed: this run must take the fresh-scan path"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "links must be cleared when the config tokens go to zero"
        );
        assert_eq!(
            db.reads()
                .get_metadata(CONFIG_RAW_CACHE_KEY)
                .unwrap()
                .as_deref(),
            Some("[]"),
            "premise: the recorded token cache is the empty list (fast-path trigger)"
        );

        // 快路径轮：签名未变 + 缓存 token 为空 + 批次非空（源码编辑）。
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();
        std::fs::write(
            project.join("src/other.py"),
            "def other_handler():\n    return 2\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config files unchanged: the zero-token fast path round keeps the signature"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "the zero-token fast path must leave zero config refs in place"
        );
    }
}
