use super::*;
fn fixture() -> (tempfile::TempDir, Plan) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let reference = root.join("reference");
    std::fs::create_dir(&reference).unwrap();
    std::fs::write(reference.join("a.rs"), "alpha_on\n").unwrap();
    std::fs::write(reference.join("b.rs"), "beta_on\n").unwrap();
    let controls = vec![
        Control {
            id: "alpha".into(),
            path: "a.rs".into(),
            on_text: "alpha_on".into(),
            off_text: "alpha_off".into(),
        },
        Control {
            id: "beta".into(),
            path: "b.rs".into(),
            on_text: "beta_on".into(),
            off_text: "beta_off".into(),
        },
    ];
    let mut variants = Vec::new();
    for bits in 0..4 {
        let id = format!("cell{bits}");
        let source = root.join(&id);
        std::fs::create_dir(&source).unwrap();
        let mut enabled = Vec::new();
        for (i, c) in controls.iter().enumerate() {
            let on = bits & (1 << i) != 0;
            if on {
                enabled.push(c.id.clone());
            }
            std::fs::write(
                source.join(&c.path),
                format!("{}\n", if on { &c.on_text } else { &c.off_text }),
            )
            .unwrap();
        }
        let binary = root.join(format!("{id}.bin"));
        std::fs::write(&binary, b"fake-validation-only-not-executed").unwrap();
        let receipt = root.join(format!("{id}.json"));
        report::json(
            &receipt,
            &BuildReceipt {
                source_files: inventory(&source).unwrap(),
                binary_sha256: sha(&std::fs::read(&binary).unwrap()),
                // Distinct positional target dir per cell; all other options
                // are semantically identical across cells. The fixture carries
                // the full required semantic key set (so the required-key
                // completeness check is exercised) plus the fixture-only
                // optional `compiler` key used by compiler-drift tests.
                build_options: json!({
                    "command":"cargo build --release --features fixture",
                    "cargo":"cargo 1.98.0-fixture",
                    "rustc":"rustc 1.98.0-fixture",
                    "SDKROOT":"fixture-sdk",
                    "RUSTFLAGS":"fixture-flags",
                    "profile":"release",
                    "features":"default",
                    "jobs":8,
                    "binding":"fixture-binding",
                    "compiler":"fixture-not-executed",
                    "CARGO_TARGET_DIR": root.join(format!("{id}-target")).display().to_string()
                }),
                exit_code: 0,
            },
        )
        .unwrap();
        variants.push(Variant {
            id,
            enabled,
            source_root: source,
            binary,
            build_receipt: receipt,
        });
    }
    let suite = Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/manifests/p0-smoke.json");
    let plan = Plan {
        schema_version: 1,
        reference_source: reference,
        controls,
        variants,
        datasets: vec![Dataset {
            id: "smoke".into(),
            suite,
            hints: Hints::default(),
        }],
        seed: 27,
    };
    (dir, plan)
}
#[test]
fn exact_source_controls_reject_undeclared_edits_and_files() {
    let (d, p) = fixture();
    validate(&p, d.path()).unwrap();
    std::fs::write(p.variants[0].source_root.join("extra.rs"), "hidden change").unwrap();
    assert!(validate(&p, d.path()).is_err());
}
#[test]
fn binary_and_compiler_drift_cannot_be_compared() {
    let (d, p) = fixture();
    let mut r: BuildReceipt = manifest::json_file(&p.variants[0].build_receipt).unwrap();
    if let Value::Object(map) = &mut r.build_options {
        map.insert("compiler".into(), json!("different"));
    }
    report::json(&p.variants[0].build_receipt, &r).unwrap();
    assert!(validate(&p, d.path()).is_err());
    let (d, p) = fixture();
    std::fs::write(&p.variants[0].binary, "different binary").unwrap();
    assert!(validate(&p, d.path()).is_err());
}
/// Mutate exactly one semantic option field in one cell's receipt.
fn with_cell_option(
    plan: &Plan,
    cell: usize,
    mutate: impl FnOnce(&mut serde_json::Map<String, Value>),
) -> BuildReceipt {
    let mut receipt: BuildReceipt =
        manifest::json_file(&plan.variants[cell].build_receipt).unwrap();
    let Value::Object(map) = &mut receipt.build_options else {
        panic!("fixture build options must be an object");
    };
    mutate(map);
    receipt
}
#[test]
fn identical_options_except_target_dir_pass_with_positional_metrics() {
    let (d, p) = fixture();
    let projection = validate(&p, d.path()).unwrap();
    assert_eq!(projection.projection_kind, SEMANTIC_PROJECTION_KIND);
    assert_eq!(projection.projection_version, SEMANTIC_PROJECTION_VERSION);
    assert_eq!(projection.positional_metrics.len(), p.variants.len());
    for v in &p.variants {
        let expected = root_target(&d, &v.id);
        assert_eq!(
            projection.positional_metrics.get(&v.id),
            Some(&expected),
            "derived positional metric must record the cell's own target dir"
        );
    }
    let distinct: BTreeSet<_> = projection.positional_metrics.values().collect();
    assert_eq!(distinct.len(), p.variants.len(), "targets must be pairwise distinct");
}
fn root_target(dir: &tempfile::TempDir, id: &str) -> String {
    dir.path().join(format!("{id}-target")).display().to_string()
}
#[test]
fn rustc_version_drift_is_rejected_by_semantic_projection() {
    let (d, p) = fixture();
    let r = with_cell_option(&p, 0, |map| {
        map.insert("rustc".into(), json!("rustc 1.98.0 (different)"));
    });
    report::json(&p.variants[0].build_receipt, &r).unwrap();
    let err = validate(&p, d.path()).unwrap_err().to_string();
    assert!(err.contains("'rustc'"), "error must name the field: {err}");
}
#[test]
fn rustflags_drift_is_rejected_by_semantic_projection() {
    let (d, p) = fixture();
    let r = with_cell_option(&p, 1, |map| {
        map.insert("RUSTFLAGS".into(), json!("-C target-cpu=native"));
    });
    report::json(&p.variants[1].build_receipt, &r).unwrap();
    let err = validate(&p, d.path()).unwrap_err().to_string();
    assert!(err.contains("'RUSTFLAGS'"), "error must name the field: {err}");
}
#[test]
fn profile_features_jobs_binding_drift_is_rejected() {
    for (key, value, cell) in [
        ("profile", json!("debug"), 0),
        ("features", json!("no-default-features"), 1),
        ("jobs", json!(4), 2),
        ("binding", json!("shared target"), 3),
    ] {
        let (d, p) = fixture();
        let key = key.to_string();
        let r = with_cell_option(&p, cell, |map| {
            map.insert(key.clone(), value.clone());
        });
        report::json(&p.variants[cell].build_receipt, &r).unwrap();
        let err = validate(&p, d.path()).unwrap_err().to_string();
        assert!(
            err.contains(&format!("'{key}'")),
            "error must name the mutated field '{key}': {err}"
        );
    }
}
#[test]
fn unknown_build_option_key_fails_closed() {
    let (d, p) = fixture();
    let r = with_cell_option(&p, 2, |map| {
        map.insert("CARGO_ENCODED_RUSTFLAGS".into(), json!("smuggled"));
    });
    report::json(&p.variants[2].build_receipt, &r).unwrap();
    let err = validate(&p, d.path()).unwrap_err().to_string();
    assert!(err.contains("unknown build option key"), "error: {err}");
    assert!(err.contains("CARGO_ENCODED_RUSTFLAGS"), "error: {err}");
}
#[test]
fn missing_target_dir_is_rejected() {
    let (d, p) = fixture();
    let r = with_cell_option(&p, 0, |map| {
        map.remove("CARGO_TARGET_DIR");
    });
    report::json(&p.variants[0].build_receipt, &r).unwrap();
    let err = validate(&p, d.path()).unwrap_err().to_string();
    assert!(err.contains("CARGO_TARGET_DIR missing"), "error: {err}");
}
/// Residual-risk closure: when every cell's receipt consistently drops the same
/// required semantic key, cross-cell projection equality alone cannot notice it;
/// per-cell required-key completeness must reject it and name the missing key.
#[test]
fn consistently_missing_required_semantic_key_is_rejected() {
    for key in REQUIRED_SEMANTIC_OPTION_KEYS {
        let (d, p) = fixture();
        for (i, v) in p.variants.iter().enumerate() {
            let r = with_cell_option(&p, i, |map| {
                map.remove(*key);
            });
            report::json(&v.build_receipt, &r).unwrap();
        }
        let err = validate(&p, d.path()).unwrap_err().to_string();
        assert!(
            err.contains(&format!("'{key}'")),
            "missing required key '{key}' must be named in the error: {err}"
        );
        assert!(
            err.contains("required build option key"),
            "rejection must be attributed to required-key completeness: {err}"
        );
    }
}
#[test]
fn shared_target_dir_across_cells_is_rejected() {
    let (d, p) = fixture();
    let shared = root_target(&d, "cell0");
    let r = with_cell_option(&p, 1, |map| {
        map.insert("CARGO_TARGET_DIR".into(), json!(shared));
    });
    report::json(&p.variants[1].build_receipt, &r).unwrap();
    let err = validate(&p, d.path()).unwrap_err().to_string();
    assert!(
        err.contains("share a CARGO_TARGET_DIR"),
        "shared target must be rejected: {err}"
    );
}
#[test]
fn unknown_projection_kind_or_version_is_rejected() {
    for marker in [
        json!({"kind":"unknown-projection-kind","version":1}),
        json!({"kind":SEMANTIC_PROJECTION_KIND,"version":SEMANTIC_PROJECTION_VERSION + 1}),
        json!({"kind":SEMANTIC_PROJECTION_KIND}),
    ] {
        let (d, p) = fixture();
        let r = with_cell_option(&p, 0, |map| {
            map.insert("semantic_projection".into(), marker.clone());
        });
        report::json(&p.variants[0].build_receipt, &r).unwrap();
        let err = validate(&p, d.path()).unwrap_err().to_string();
        assert!(
            err.contains("unsupported build options semantic projection"),
            "marker {marker} must be rejected: {err}"
        );
    }
    // The supported marker itself is accepted.
    let (d, p) = fixture();
    let r = with_cell_option(&p, 0, |map| {
        map.insert(
            "semantic_projection".into(),
            json!({"kind":SEMANTIC_PROJECTION_KIND,"version":SEMANTIC_PROJECTION_VERSION}),
        );
    });
    report::json(&p.variants[0].build_receipt, &r).unwrap();
    assert!(validate(&p, d.path()).is_ok());
}
#[test]
fn matrix_requires_every_cell_and_unique_factor_paths() {
    let (d, p) = fixture();
    let mut bad = p.clone();
    bad.variants.pop();
    assert!(validate(&bad, d.path()).is_err());
    let mut bad = p.clone();
    bad.variants[0].enabled = bad.variants[1].enabled.clone();
    assert!(validate(&bad, d.path()).is_err());
    let mut bad = p;
    bad.controls[1].path = bad.controls[0].path.clone();
    assert!(validate(&bad, d.path()).is_err());
}
#[test]
fn malformed_plan_and_hints_fail_closed() {
    let (d, p) = fixture();
    let mut bad = p.clone();
    bad.datasets[0].hints.pinned_files = vec!["../outside.py".into()];
    assert!(validate(&bad, d.path()).is_err());
    let mut bad = p.clone();
    bad.controls[0].on_text = "absent anchor".into();
    assert!(validate(&bad, d.path()).is_err());
    let mut bad = p;
    bad.datasets[0].hints.file_preselect_limit = Some(0);
    assert!(validate(&bad, d.path()).is_err());
}
