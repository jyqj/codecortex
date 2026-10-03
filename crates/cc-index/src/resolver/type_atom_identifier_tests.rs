//! Synthetic-only regression evidence shared unchanged across the three revisions.
use super::type_atoms;
use crate::resolver::SymbolCatalog;
use cc_model::edge::SemanticRelation;
use cc_model::resolution::{resolution_name_keys, DependencyKind, ResolutionManifest};
use cc_model::Language;
use cc_parsers::ParserRegistry;

#[test]
fn parser_identifiers_keep_uses_type_and_name_dependencies() {
    let mut failures = Vec::new();
    for (language, path, names) in [
        (
            Language::Python,
            "synthetic.py",
            vec!["_", "__", "℘", "℮", "ᢅ", "ᢆ", "A\u{301}", "数据"],
        ),
        (
            Language::TypeScript,
            "synthetic.ts",
            vec!["_", "__", "$", "$$", "_$", "$_", "℘", "A\u{301}"],
        ),
    ] {
        for name in names {
            let source = if language == Language::Python {
                format!(
                    "class {name}:\n    pass\ndef consume(value: {name}) -> {name}:\n    pass\n"
                )
            } else {
                format!(
                    "class {name} {{}}\nfunction consume(value: {name}): {name} {{ throw 0; }}\n"
                )
            };
            let mut outcome = ParserRegistry::new()
                .parse(path, &source, language)
                .unwrap();
            let target = outcome.symbols.iter().find(|s| s.name == name).unwrap();
            let uid = target.symbol_uid.clone().expect("parser identity");
            let consumer = outcome
                .symbols
                .iter()
                .find(|s| s.name == "consume")
                .unwrap();
            assert_eq!(
                consumer.return_type.as_deref(),
                Some(name),
                "{path}: {name}"
            );
            let mut catalog = SymbolCatalog::new();
            catalog.add_symbols(&outcome.symbols);
            catalog.derive_uses_type_edges(path, &mut outcome);
            catalog.resolve_outcome(path, &mut outcome);
            let edge = outcome.semantic_edges.iter().any(|e| {
                e.source_symbol == "consume"
                    && e.target_symbol == name
                    && e.target_symbol_uid.as_ref() == Some(&uid)
                    && e.relation_kind == SemanticRelation::UsesType
            });
            let dependency = outcome
                .resolution
                .dependencies
                .iter()
                .any(|d| d.kind == DependencyKind::NameBucket && d.key == name.to_lowercase());
            println!("{path} {name:?}: uses_type={edge} name_bucket={dependency}");
            outcome.resolution.validate().unwrap();
            if !edge || !dependency {
                failures.push(format!(
                    "{path} {name:?}: edge={edge}, dependency={dependency}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn unicode_identifier_start_domain_is_preserved() {
    // The Python grammar uses [_\p{XID_Start}][_\p{XID_Continue}]*.
    // Check every scalar in XID_Start, not a hand-picked Unicode exception list.
    // ASCII uppercase single-character type parameters remain intentionally excluded.
    let starts = regex::Regex::new(r"^\p{XID_Start}$").unwrap();
    let mut dropped = Vec::new();
    for value in 0..=0x10ffff {
        let Some(ch) = char::from_u32(value) else {
            continue;
        };
        if ch.is_ascii_uppercase() {
            continue;
        }
        let name = ch.to_string();
        if starts.is_match(&name) && type_atoms(&name) != [name.clone()] {
            dropped.push(name);
        }
    }
    println!("dropped XID_Start scalars: {dropped:?}");
    assert!(dropped.is_empty());
    for length in 1..=8 {
        for seed in ["_", "$", "_$", "$_"] {
            let name = seed.repeat(length);
            assert_eq!(type_atoms(&name), [name]);
        }
    }
}

#[test]
fn qualified_generic_union_atoms() {
    assert_eq!(
        type_atoms("Box[pkg._, pkg::__] | Wrap<$, ℘> | 数据.类型"),
        ["Box", "pkg._", "pkg::__", "Wrap", "$", "℘", "数据.类型"]
    );
}

#[test]
fn parsed_variadic_tuple_keeps_names_without_pseudo_atom() {
    let path = "synthetic.py";
    let source = "class Widget:\n    pass\ndef consume(value: tuple[Widget, ...]):\n    pass\n";
    let mut outcome = ParserRegistry::new()
        .parse(path, source, Language::Python)
        .unwrap();
    let mut catalog = SymbolCatalog::new();
    catalog.add_symbols(&outcome.symbols);
    catalog.derive_uses_type_edges(path, &mut outcome);
    catalog.resolve_outcome(path, &mut outcome);
    let atoms: Vec<_> = outcome
        .semantic_edges
        .iter()
        .filter(|e| e.source_symbol == "consume" && e.relation_kind == SemanticRelation::UsesType)
        .map(|e| e.target_symbol.as_str())
        .collect();
    assert_eq!(atoms, ["tuple", "Widget"]);
    assert!(!outcome
        .resolution
        .dependencies
        .iter()
        .any(|d| d.key.is_empty()));
    outcome.resolution.validate().unwrap();
}

#[test]
fn punctuation_atoms_remain_excluded() {
    assert_eq!(type_atoms("tuple[Widget, ...]"), ["tuple", "Widget"]);
    for raw in ["...", "::", "?", "&", "***", "", " \t\n"] {
        assert!(type_atoms(raw).is_empty(), "{raw:?}");
    }
    for ch in (0..=127).filter_map(char::from_u32) {
        if ch.is_ascii_punctuation() && !matches!(ch, '_' | '$') {
            assert!(type_atoms(&ch.to_string().repeat(3)).is_empty(), "{ch:?}");
        }
    }
}

#[test]
fn qualified_name_keys_never_create_empty_dependencies() {
    for raw in [
        "_", "__", "$", "℘", "pkg._", "pkg::__", "pkg.$", "pkg.℘", "pkg.", "pkg::", "...",
    ] {
        let keys = resolution_name_keys(raw);
        assert!(keys.contains(&raw.to_lowercase()));
        if let Some((_, leaf)) = raw.rsplit_once('.').or_else(|| raw.rsplit_once("::")) {
            if !leaf.is_empty() {
                assert!(keys.contains(&leaf.to_lowercase()));
            }
        }
        assert!(!keys.contains(""), "{raw:?}");
        let mut manifest = ResolutionManifest::new();
        for key in keys {
            manifest.dependency(DependencyKind::NameBucket, key);
        }
        manifest.validate().unwrap();
    }
}

#[test]
fn explicit_empty_dependency_remains_strictly_invalid() {
    let mut invalid = ResolutionManifest::new();
    invalid.dependency(DependencyKind::NameBucket, "");
    assert!(invalid.validate().is_err());
    invalid.normalize();
    assert!(invalid.validate().is_err());
    let roundtrip: ResolutionManifest =
        serde_json::from_str(&serde_json::to_string(&invalid).unwrap()).unwrap();
    assert!(roundtrip.validate().is_err());
}
