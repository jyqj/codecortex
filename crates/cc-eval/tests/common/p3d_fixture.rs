use std::path::Path;
#[derive(Clone)]
pub struct Lookup {
    pub language: &'static str,
    pub importer: String,
    pub spec: String,
    pub target: String,
}
pub fn put(root: &Path, file: &str, text: &str) {
    let p = root.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
/// Hand-authored target identities, independent of resolver output.
pub fn fixture(root: &Path, n: usize) -> Vec<Lookup> {
    put(
        root,
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    );
    put(
        root,
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    put(root, "package.json", r#"{"workspaces":["ts/*"]}"#);
    let mut lookups = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        put(
            root,
            &format!("ts/p{i}/package.json"),
            &format!(r#"{{"name":"@t/p{i}","exports":"./api.ts"}}"#),
        );
        put(
            root,
            &format!("ts/p{i}/api.ts"),
            "export function marker(){return 1;}\n",
        );
        let importer = format!("ts/p{i}/use.ts");
        let spec = format!("@t/p{j}");
        put(
            root,
            &importer,
            &format!(
                "import {{marker}} from '{spec}'; export function entry(){{return marker();}}\n"
            ),
        );
        lookups.push(Lookup {
            language: "typescript",
            importer,
            spec,
            target: format!("ts/p{j}/api.ts"),
        });
        put(
            root,
            &format!("py/p{i}/pyproject.toml"),
            "[tool.setuptools.package-dir]\n\"\" = \"src\"\n",
        );
        put(root, &format!("py/p{i}/src/pkg/__init__.py"), "");
        put(
            root,
            &format!("py/p{i}/src/pkg/api.py"),
            "def marker():\n    return 1\n",
        );
        let importer = format!("py/p{i}/src/pkg/use.py");
        put(
            root,
            &importer,
            "from .api import marker\ndef entry():\n    return marker()\n",
        );
        lookups.push(Lookup {
            language: "python",
            importer,
            spec: ".api".into(),
            target: format!("py/p{i}/src/pkg/api.py"),
        });
        put(
            root,
            &format!("go/p{i}/go.mod"),
            &format!("module example.com/g{i}\ngo 1.22\n"),
        );
        put(
            root,
            &format!("go/p{i}/api/api.go"),
            "package api\nfunc Marker() int {return 1}\n",
        );
        let importer = format!("go/p{i}/main.go");
        let spec = format!("example.com/g{i}/api");
        put(
            root,
            &importer,
            &format!("package app\nimport \"{spec}\"\nfunc Entry() int {{return api.Marker()}}\n"),
        );
        lookups.push(Lookup {
            language: "go",
            importer,
            spec,
            target: format!("go/p{i}/api/api.go"),
        });
        put(
            root,
            &format!("rust/p{i}/Cargo.toml"),
            &format!("[package]\nname=\"r{i}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"),
        );
        put(
            root,
            &format!("rust/p{i}/src/api.rs"),
            "pub fn marker()->i32 {1}\n",
        );
        let importer = format!("rust/p{i}/src/lib.rs");
        put(
            root,
            &importer,
            "mod api;\nuse crate::api::marker;\npub fn entry()->i32{marker()}\n",
        );
        lookups.push(Lookup {
            language: "rust",
            importer,
            spec: "crate::api::marker".into(),
            target: format!("rust/p{i}/src/api.rs"),
        });
    }
    lookups
}
