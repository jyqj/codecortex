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
                build_options: json!({"compiler":"fixture-not-executed"}),
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
    r.build_options = json!({"compiler":"different"});
    report::json(&p.variants[0].build_receipt, &r).unwrap();
    assert!(validate(&p, d.path()).is_err());
    let (d, p) = fixture();
    std::fs::write(&p.variants[0].binary, "different binary").unwrap();
    assert!(validate(&p, d.path()).is_err());
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
