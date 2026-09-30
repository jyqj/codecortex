//! Package input projection preserves condition-key source order. No package code is run.
use cc_model::{module_inputs::*, CcError, CcResult};
use serde::de::{Deserialize, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use std::{collections::BTreeSet, fmt};
struct Ordered(PackageTarget);
impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded JSON value")
            }
            fn visit_unit<E: Error>(self) -> Result<Ordered, E> {
                Ok(Ordered(PackageTarget::Null))
            }
            fn visit_none<E: Error>(self) -> Result<Ordered, E> {
                self.visit_unit()
            }
            fn visit_str<E: Error>(self, s: &str) -> Result<Ordered, E> {
                Ok(Ordered(PackageTarget::Path(s.into())))
            }
            fn visit_string<E: Error>(self, s: String) -> Result<Ordered, E> {
                Ok(Ordered(PackageTarget::Path(s)))
            }
            fn visit_bool<E: Error>(self, _: bool) -> Result<Ordered, E> {
                Ok(Ordered(PackageTarget::Invalid))
            }
            fn visit_i64<E: Error>(self, _: i64) -> Result<Ordered, E> {
                Ok(Ordered(PackageTarget::Invalid))
            }
            fn visit_u64<E: Error>(self, _: u64) -> Result<Ordered, E> {
                Ok(Ordered(PackageTarget::Invalid))
            }
            fn visit_f64<E: Error>(self, _: f64) -> Result<Ordered, E> {
                Ok(Ordered(PackageTarget::Invalid))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Ordered, A::Error> {
                let mut v = Vec::new();
                while let Some(Ordered(x)) = a.next_element()? {
                    if v.len() >= 4096 {
                        return Err(A::Error::custom("package array limit"));
                    }
                    v.push(x);
                }
                Ok(Ordered(PackageTarget::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Ordered, A::Error> {
                let mut v = Vec::new();
                let mut seen = BTreeSet::new();
                while let Some((k, Ordered(x))) = a.next_entry::<String, Ordered>()? {
                    if v.len() >= 4096 || !seen.insert(k.clone()) {
                        return Err(A::Error::custom("package duplicate key or key limit"));
                    }
                    v.push((k, x));
                }
                Ok(Ordered(PackageTarget::Object(v)))
            }
        }
        d.deserialize_any(V)
    }
}
pub(super) fn parse(bytes: &[u8]) -> CcResult<PackageConfig> {
    let Ordered(raw) = serde_json::from_slice::<Ordered>(bytes)
        .map_err(|_| CcError::Config("invalid_or_duplicate_package_json".into()))?;
    let PackageTarget::Object(fields) = raw else {
        return Err(CcError::Config("package_json_must_be_object".into()));
    };
    let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v);
    let string = |k: &str| match get(k) {
        Some(PackageTarget::Path(v)) => Some(v.clone()),
        _ => None,
    };
    let mut p = PackageConfig {
        name: string("name"),
        package_type: string("type"),
        main: string("main"),
        types: string("types").or_else(|| string("typings")),
        exports: get("exports").cloned().unwrap_or_default(),
        imports: get("imports").cloned().unwrap_or_default(),
        ..Default::default()
    };
    for k in ["name", "main", "types", "typings", "type"] {
        if get(k).is_some() && string(k).is_none() {
            p.diagnostics.push(format!("invalid_package_{k}"));
        }
    }
    if p.package_type
        .as_deref()
        .is_some_and(|t| !matches!(t, "module" | "commonjs"))
    {
        p.diagnostics.push("invalid_package_type".into());
    }
    if get("typesVersions").is_some() {
        p.diagnostics.push("types_versions_unsupported".into());
    }
    let workspace = match get("workspaces") {
        Some(PackageTarget::Object(v)) => v.iter().find(|(k, _)| k == "packages").map(|(_, v)| v),
        v => v,
    };
    if let Some(w) = workspace {
        if let PackageTarget::Array(a) = w {
            for v in a {
                if let PackageTarget::Path(s) = v {
                    if s.len() <= 4096 && !s.contains(['!', '[', ']', '{', '}', '\\', ':']) {
                        p.workspaces.push(s.clone());
                        continue;
                    }
                }
                p.diagnostics.push("workspace_pattern_unsupported".into());
            }
        } else {
            p.diagnostics.push("invalid_workspaces".into());
        }
    }
    for field in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(PackageTarget::Object(v)) = get(field) {
            for (k, v) in v {
                if let PackageTarget::Path(s) = v {
                    p.dependencies.entry(k.clone()).or_insert_with(|| s.clone());
                }
            }
        }
    }
    Ok(p)
}
/// Segment-based glob over an already admitted manifest inventory. Never walks.
pub(crate) fn member_matches(pattern: &str, path: &str) -> bool {
    let p = pattern
        .trim_start_matches("./")
        .trim_end_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    let s = path.split('/').collect::<Vec<_>>();
    if p.len() > 32 || s.len() > 32 {
        return false;
    }
    let mut dp = vec![vec![false; s.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 0..p.len() {
        for j in 0..=s.len() {
            if p[i] == "**" {
                dp[i + 1][j] |= dp[i][j];
                if j > 0 {
                    dp[i + 1][j] |= dp[i + 1][j - 1];
                }
            } else if j < s.len() && (p[i] == "*" || p[i] == s[j]) {
                dp[i + 1][j + 1] |= dp[i][j];
            }
        }
    }
    dp[p.len()][s.len()]
}
