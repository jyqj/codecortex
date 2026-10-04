fn public_cap(
    root: &Path,
) -> std::result::Result<
    cc_index::project_model::python_inventory::CapturedDeclarations,
    cc_index::project_model::python_inventory::CaptureRefusal,
> {
    let p = policy();
    cc_index::project_model::python_inventory::capture_python_declarations(
        root,
        cc_index::project_model::python_inventory::CaptureRequest {
            owner: "independent-owner",
            scope: cc_index::project_model::python_inventory::AuthorizedScope::EntireProject,
            exclusions: &[],
        },
        cc_index::project_model::python_inventory::AdmissionPolicies {
            capture: cc_index::project_model::python_inventory::CaptureLimits {
                entries: p.capture.entries,
                files: p.capture.files,
                total_bytes: p.capture.total_bytes,
                file_bytes: p.capture.file_bytes,
                depth: p.capture.depth,
                path_bytes: p.capture.path_bytes,
                total_path_bytes: p.capture.total_path_bytes,
                output_count: p.capture.output_count,
                output_bytes: p.capture.output_bytes,
            },
            ast: p.ast,
            declarations: p.declarations,
        },
    )
}
use super::*;
use std::{
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{symlink, PermissionsExt},
    },
    path::Path,
};
const CFG: &[u8] = b"# independent\n[tool.setuptools.package-dir]\n\"\"='./lib'\n[tool.setuptools.packages.find]\nwhere=['lib/./','lib']\n";
fn policy() -> AdmissionPolicies {
    AdmissionPolicies {
        capture: CaptureLimits {
            entries: 128,
            files: 64,
            total_bytes: 16384,
            file_bytes: 4096,
            depth: 16,
            path_bytes: 512,
            total_path_bytes: 16384,
            output_count: 64,
            output_bytes: 262144,
        },
        ast: PythonIdentityLimits {
            source_bytes: 4096,
            visited_nodes: 4096,
            tree_depth: 64,
            declarations: 64,
            output_segments: 256,
            output_text_bytes: 16384,
            parse_timeout_micros: 1_000_000,
        },
        declarations: DeclarationLimits {
            max_files: 64,
            max_total_bytes: 16384,
            max_file_bytes: 4096,
            max_roots: 1,
            max_evidence: 16,
            max_ancestry_depth: 16,
            max_identifier_bytes: 128,
        },
    }
}
fn put(root: &Path, path: &str, bytes: &[u8]) {
    fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    fs::write(root.join(path), bytes).unwrap();
}
fn populate(root: &Path) {
    put(root, "pyproject.toml", CFG);
    put(
        root,
        "lib/reef/__init__.py",
        b"# marker\r\ndef born(): pass\r\n",
    );
    put(root,"lib/reef/wave.py", b"\xef\xbb\xbf# exact bytes\r\nclass Tide:\r\n    def rise(self): pass\r\ndef factory():\r\n    def local(): pass\r\n");
    put(root, "lib/empty.py", b"# zero declarations\n");
    put(root, ".git/HEAD", b"ref: refs/heads/review\n");
    put(root, ".cache/data", b"opaque\x00\xff");
    put(root, ".hidden", b"hidden");
    put(root, ".gitignore", b".cache\n__init__.py\n");
    fs::create_dir_all(root.join("vacant")).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    populate(t.path());
    t
}
fn req() -> CaptureRequest<'static> {
    CaptureRequest {
        owner: "independent-owner",
        scope: AuthorizedScope::EntireProject,
        exclusions: &[],
    }
}
fn cap(root: &Path, p: AdmissionPolicies) -> Result<CapturedDeclarations> {
    capture_python_declarations(root, req(), p)
}
fn fp(c: &CapturedDeclarations) -> String {
    match &c.outcomes()["lib/reef/wave.py"][0] {
        IdentityOutcome::Derived(d) => d.fingerprint().unwrap(),
        x => panic!("{x:?}"),
    }
}
#[test]
fn review_complete_bytes_evidence_public_equivalence() {
    let t = fixture();
    let c = cap(t.path(), policy()).unwrap();
    assert_eq!(c.inventory().len(), 8);
    for k in [
        ".git/HEAD",
        ".cache/data",
        ".hidden",
        "lib/reef/__init__.py",
    ] {
        assert_eq!(c.inventory()[k], fs::read(t.path().join(k)).unwrap());
    }
    assert_eq!(c.provenance().roots["lib"].len(), 3);
    assert_eq!(c.config().digest, Some(content_digest(CFG)));
    assert_eq!(c.outcomes()["lib/empty.py"].len(), 0);
    let d = match &c.outcomes()["lib/reef/wave.py"][1] {
        IdentityOutcome::Derived(d) => d,
        _ => panic!(),
    };
    assert_eq!(
        d.binding().source_digest,
        content_digest(&c.inventory()["lib/reef/wave.py"])
    );
    assert_eq!(
        d.binding().root.directive,
        serde_json::to_string(&c.provenance().roots["lib"]).unwrap()
    );
    for a in &d.binding().ancestry {
        assert_eq!(
            &c.inventory()["lib/reef/wave.py"][a.name_span.start..a.name_span.end],
            a.name.as_bytes()
        );
    }
    let real = public_cap(t.path()).unwrap();
    assert_eq!(c.outcomes(), real.outcomes());
    assert_eq!(c.inventory(), real.inventory());
}
#[test]
fn review_all_capture_boundaries_and_json_accounting() {
    let t = fixture();
    let c = cap(t.path(), policy()).unwrap();
    let mut p = policy();
    let dirs = ["lib", "lib/reef", ".git", ".cache", "vacant"];
    p.capture = CaptureLimits {
        entries: c.inventory().len() + dirs.len() + 1,
        files: c.inventory().len(),
        total_bytes: c.inventory().values().map(Vec::len).sum(),
        file_bytes: c.inventory().values().map(Vec::len).max().unwrap(),
        depth: 3,
        path_bytes: c.inventory().keys().map(String::len).max().unwrap(),
        total_path_bytes: c.inventory().keys().map(String::len).sum::<usize>()
            + dirs.iter().map(|s| s.len()).sum::<usize>(),
        output_count: c.outcomes().values().map(Vec::len).sum(),
        output_bytes: serde_json::to_vec(c.outcomes()).unwrap().len(),
    };
    cap(t.path(), p).unwrap();
    for resource in [
        "entries",
        "files",
        "total_bytes",
        "file_bytes",
        "depth",
        "path_bytes",
        "total_path_bytes",
        "output_count",
        "output_bytes",
    ] {
        let mut q = p;
        match resource {
            "entries" => q.capture.entries -= 1,
            "files" => q.capture.files -= 1,
            "total_bytes" => q.capture.total_bytes -= 1,
            "file_bytes" => q.capture.file_bytes -= 1,
            "depth" => q.capture.depth -= 1,
            "path_bytes" => q.capture.path_bytes -= 1,
            "total_path_bytes" => q.capture.total_path_bytes -= 1,
            "output_count" => q.capture.output_count -= 1,
            _ => q.capture.output_bytes -= 1,
        }
        assert!(
            matches!(cap(t.path(),q),Err(CaptureRefusal::Budget(s)) if s==resource),
            "{resource}"
        );
    }
}
#[test]
fn review_full_preflight_precedes_ast_and_config() {
    let t = fixture();
    put(t.path(), "pyproject.toml", b"invalid = [");
    put(t.path(), "lib/reef/wave.py", b"def broken(:\n");
    let mut p = policy();
    p.capture.files = 7;
    assert!(matches!(
        cap(t.path(), p),
        Err(CaptureRefusal::Budget("files"))
    ));
    let mut p = policy();
    p.declarations.max_total_bytes = 1;
    assert!(matches!(
        cap(t.path(), p),
        Err(CaptureRefusal::Model(DeclarationError::Resource(_)))
    ));
}
#[test]
fn review_nonregular_and_native_alias_fixtures() {
    for kind in 0..10 {
        let t = fixture();
        let outer = tempfile::tempdir().unwrap();
        match kind {
            0 => symlink("reef", t.path().join("lib/link")).unwrap(),
            1 => symlink("wave.py", t.path().join("lib/reef/link.py")).unwrap(),
            2 => fs::hard_link(t.path().join(".hidden"), outer.path().join("external")).unwrap(),
            3 => put(t.path(), ".HIDDEN", b""),
            4 => {
                fs::write(
                    t.path().join(std::ffi::OsStr::from_bytes(b"native\xff")),
                    b"",
                )
                .unwrap();
            }
            5 => put(t.path(), "LPT9.txt", b""),
            6 => put(t.path(), "name\\alias", b""),
            7 => put(t.path(), "stream:alias", b""),
            8 => put(t.path(), "unicode-é", b""),
            _ => {
                let n =
                    std::ffi::CString::new(t.path().join("pipe").as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(n.as_ptr(), 0o600) }, 0);
            }
        }
        let e = cap(t.path(), policy()).unwrap_err();
        assert!(
            matches!(
                e,
                CaptureRefusal::SymlinkOrNonregular | CaptureRefusal::Alias | CaptureRefusal::Path
            ),
            "case {kind}: {e:?}"
        );
    }
    let t = fixture();
    let o = tempfile::tempdir().unwrap();
    symlink(t.path(), o.path().join("alias")).unwrap();
    assert_eq!(
        cap(&o.path().join("alias"), policy()).unwrap().outcomes(),
        cap(t.path(), policy()).unwrap().outcomes()
    );
}
#[test]
fn review_marker_absence_collision_and_git_cache_invalidation() {
    let t = fixture();
    let first = cap(t.path(), policy()).unwrap();
    let before = fp(&first);
    for p in [
        ".git/HEAD",
        ".cache/data",
        "pyproject.toml",
        "lib/reef/__init__.py",
    ] {
        let saved = fs::read(t.path().join(p)).unwrap();
        let mut next = saved.clone();
        next.extend_from_slice(b"\n# change\n");
        put(t.path(), p, &next);
        assert_ne!(before, fp(&cap(t.path(), policy()).unwrap()), "{p}");
        put(t.path(), p, &saved);
    }
    fs::remove_file(t.path().join("lib/reef/__init__.py")).unwrap();
    assert_eq!(
        cap(t.path(), policy()).unwrap().outcomes()["lib/reef/wave.py"][0],
        IdentityOutcome::Unavailable(IdentityReason::NamespaceAncestry)
    );
    put(t.path(), "lib/reef/__init__.py", b"pass\n");
    put(t.path(), "lib/reef.py", b"def other(): pass\n");
    assert_eq!(
        cap(t.path(), policy()).unwrap().outcomes()["lib/reef/wave.py"][0],
        IdentityOutcome::Unavailable(IdentityReason::ModulePackageCollision)
    );
    assert_eq!(before, fp(&first));
}
#[test]
fn review_default_partial_selector_refusal() {
    for text in [
        "[project]\nname='review'\n",
        "[tool.setuptools.package-dir]\n\"\"='lib'\nnamed='lib'\n",
        "[tool.setuptools.package-dir]\n\"\"='lib'\n\"\"='lib'\n",
        "[tool.setuptools.packages.find]\nwhere=['lib']\ninclude=['reef']\n",
        "[tool.setuptools.packages.find]\nwhere=['lib']\nnamespaces=false\n",
        "[tool.setuptools.package-dir]\n\"\"='lib'\n[tool.poetry]\nname='review'\n",
        "[tool.setuptools.package-dir]\n\"\"='lib'\n[tool.setuptools]\npackages=['reef']\n",
        "[tool.setuptools.packages.find]\nwhere=['lib','vacant']\n",
    ] {
        let t = fixture();
        put(t.path(), "pyproject.toml", text.as_bytes());
        assert!(
            matches!(cap(t.path(), policy()), Err(CaptureRefusal::Configuration)),
            "{text}"
        );
    }
    let t = fixture();
    put(t.path(), ".cache/pyproject.toml", CFG);
    assert!(matches!(
        cap(t.path(), policy()),
        Err(CaptureRefusal::Configuration)
    ));
    for scope in [
        AuthorizedScope::Subtree("lib".into()),
        AuthorizedScope::EntireProject,
    ] {
        assert!(matches!(
            capture_python_declarations(
                t.path(),
                CaptureRequest {
                    owner: "owner",
                    scope,
                    exclusions: &[".git".into()]
                },
                policy()
            ),
            Err(CaptureRefusal::UnauthorizedScope)
        ));
    }
}
#[test]
fn review_ast_and_model_failures_discard_all_results() {
    let t = fixture();
    put(
        t.path(),
        "lib/zz.py",
        b"def valid(): pass\ndef broken(: pass\n",
    );
    assert!(matches!(
        cap(t.path(), policy()),
        Err(CaptureRefusal::Ast {
            reason: PythonIdentityReason::SyntaxError,
            ..
        })
    ));
    fs::remove_file(t.path().join("lib/zz.py")).unwrap();
    let mut p = policy();
    p.ast.source_bytes = 1;
    assert!(matches!(
        cap(t.path(), p),
        Err(CaptureRefusal::Ast {
            reason: PythonIdentityReason::SourceLimit,
            ..
        })
    ));
    let mut p = policy();
    p.declarations.max_ancestry_depth = 1;
    assert!(matches!(
        cap(t.path(), p),
        Err(CaptureRefusal::Model(DeclarationError::Resource(_)))
    ));
}
#[test]
fn review_deterministic_real_mutations_and_mixed_metadata() {
    for kind in 0..12 {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        populate(&root);
        let result = capture_inner(&root, req(), policy(), || match kind {
            0 => put(&root, "lib/reef/wave.py", b"def changed(): pass\n"),
            1 => put(
                &root,
                "pyproject.toml",
                b"# mutation\n[tool.setuptools.package-dir]\n\"\"='lib'\n",
            ),
            2 => fs::rename(
                root.join("lib/reef/wave.py"),
                root.join("lib/reef/renamed.py"),
            )
            .unwrap(),
            3 => fs::remove_file(root.join("lib/reef/__init__.py")).unwrap(),
            4 => put(&root, "lib/reef.py", b"pass\n"),
            5 => {
                fs::rename(&root, outer.path().join("old")).unwrap();
                populate(&root);
            }
            6 => {
                fs::remove_file(root.join("lib/reef/wave.py")).unwrap();
                symlink("__init__.py", root.join("lib/reef/wave.py")).unwrap();
            }
            7 => fs::rename(root.join("pyproject.toml"), root.join("renamed.toml")).unwrap(),
            8 => fs::set_permissions(root.join(".hidden"), fs::Permissions::from_mode(0o400))
                .unwrap(),
            9 => {
                fs::hard_link(root.join(".hidden"), outer.path().join("link")).unwrap();
            }
            10 => fs::create_dir(root.join("newempty")).unwrap(),
            _ => put(&root, ".git/HEAD", b"ref: refs/heads/other\n"),
        });
        assert!(result.is_err(), "mutation {kind} accepted");
        println!("mutation {kind}: {:?}", result.unwrap_err());
    }
}
#[test]
fn review_partial_find_without_where_observation() {
    let t = fixture();
    put(
        t.path(),
        "pyproject.toml",
        b"[tool.setuptools.package-dir]\n\"\"='lib'\n[tool.setuptools.packages.find]\n",
    );
    println!(
        "empty find with explicit package-dir: {:?}",
        cap(t.path(), policy()).map(|c| c.provenance().clone())
    );
}
#[test]
fn review_ast_and_model_exact_one_over_policies() {
    let t = fixture();
    for field in 0..6 {
        let set = |p: &mut AdmissionPolicies, n| match field {
            0 => p.ast.source_bytes = n,
            1 => p.ast.visited_nodes = n,
            2 => p.ast.tree_depth = n,
            3 => p.ast.declarations = n,
            4 => p.ast.output_segments = n,
            _ => p.ast.output_text_bytes = n,
        };
        let upper = match field {
            0 => 4096,
            1 => 4096,
            2 => 64,
            3 => 64,
            4 => 256,
            _ => 16384,
        };
        let (mut lo, mut hi) = (1, upper);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut p = policy();
            set(&mut p, mid);
            if cap(t.path(), p).is_ok() {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        let mut p = policy();
        set(&mut p, lo);
        cap(t.path(), p).unwrap();
        let mut p = policy();
        set(&mut p, lo - 1);
        assert!(cap(t.path(), p).is_err());
        println!("AST field {field} exact threshold {lo}; one below refused");
    }
    let c = cap(t.path(), policy()).unwrap();
    for field in 0..7 {
        let mut p = policy();
        let exact = match field {
            0 => c.inventory().len(),
            1 => c.inventory().values().map(Vec::len).sum(),
            2 => c.inventory().values().map(Vec::len).max().unwrap(),
            3 => 1,
            4 => 1,
            5 => 2,
            _ => 7,
        };
        let set = |p: &mut AdmissionPolicies, n| match field {
            0 => p.declarations.max_files = n,
            1 => p.declarations.max_total_bytes = n,
            2 => p.declarations.max_file_bytes = n,
            3 => p.declarations.max_roots = n,
            4 => p.declarations.max_evidence = n,
            5 => p.declarations.max_ancestry_depth = n,
            _ => p.declarations.max_identifier_bytes = n,
        };
        set(&mut p, exact);
        cap(t.path(), p).unwrap();
        set(&mut p, exact - 1);
        assert!(matches!(
            cap(t.path(), p),
            Err(CaptureRefusal::Model(DeclarationError::Resource(_)))
        ));
        println!("model field {field} exact threshold {exact}; one below refused");
    }
}
#[test]
fn review_empty_output_and_unavailable_json_exactness() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "pyproject.toml",
        b"[tool.setuptools.package-dir]\n\"\"='.'\n",
    );
    let mut p = policy();
    p.capture.output_count = 0;
    p.capture.output_bytes = 2;
    assert_eq!(cap(t.path(), p).unwrap().outcomes().len(), 0);
    p.capture.output_bytes = 1;
    assert!(matches!(
        cap(t.path(), p),
        Err(CaptureRefusal::Budget("output_bytes"))
    ));
    put(t.path(), "quote'name.py", b"# empty\n");
    put(t.path(), "__init__.py", b"def atroot(): pass\n");
    let c = cap(t.path(), policy()).unwrap();
    assert_eq!(
        c.outcomes()["__init__.py"][0],
        IdentityOutcome::Unavailable(IdentityReason::RootInitializer)
    );
    p = policy();
    p.capture.output_bytes = serde_json::to_vec(c.outcomes()).unwrap().len();
    p.capture.output_count = 1;
    cap(t.path(), p).unwrap();
    p.capture.output_bytes -= 1;
    assert!(matches!(
        cap(t.path(), p),
        Err(CaptureRefusal::Budget("output_bytes"))
    ));
}
#[test]
fn review_contract_partial_find_must_refuse() {
    let mut admitted = Vec::new();
    for find in [
        "[tool.setuptools.packages.find]\n",
        "[tool.setuptools.packages.find]\nwhere=[]\n",
    ] {
        let t = fixture();
        let text = format!("[tool.setuptools.package-dir]\n\"\"='lib'\n{find}");
        put(t.path(), "pyproject.toml", text.as_bytes());
        let result = cap(t.path(), policy());
        let public = public_cap(t.path());
        assert_eq!(result.is_ok(), public.is_ok());
        println!("public API partial find admission: {}", public.is_ok());
        println!(
            "partial configuration {text:?}: {:?}",
            result.as_ref().map(|c| c.provenance())
        );
        if !matches!(result, Err(CaptureRefusal::Configuration)) {
            admitted.push(text);
        }
    }
    assert!(
        admitted.is_empty(),
        "partial find unexpectedly admitted: {admitted:?}"
    );
}
#[test]
fn review_contract_native_config_backslash_must_refuse() {
    let mut admitted = Vec::new();
    for text in [
        "[tool.setuptools.package-dir]\n\"\"='lib\\reef'\n",
        "[tool.setuptools.packages.find]\nwhere=['lib\\reef']\n",
    ] {
        let t = fixture();
        put(t.path(), "pyproject.toml", text.as_bytes());
        assert!(!t.path().join(r"lib\reef").exists());
        assert!(t.path().join("lib/reef").is_dir());
        let result = cap(t.path(), policy());
        println!(
            "native config spelling {text:?}: {:?}",
            result.as_ref().map(|c| c.provenance())
        );
        if !matches!(result, Err(CaptureRefusal::Configuration)) {
            admitted.push(text);
        }
    }
    assert!(
        admitted.is_empty(),
        "non-native root unexpectedly admitted: {admitted:?}"
    );
}
