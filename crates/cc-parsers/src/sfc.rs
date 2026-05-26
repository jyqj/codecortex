//! Vue/Svelte single-file component extraction.
//!
//! This is intentionally lightweight: extract script blocks, reuse the JS/TS
//! parser with original line offsets, and add component/template references so
//! the resolver can surface SFC relationships to LLM tools.

use crate::jsts::JsTsParser;
use crate::traits::FileParser;
use cc_model::edge::{CallEdgeRecord, DispatchKind, ResolutionKind};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

static VUE_SCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script(?P<attrs>[^>]*)>(?P<body>.*?)</script>"#).expect("vue script regex")
});
static SVELTE_SCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script(?P<attrs>[^>]*)>(?P<body>.*?)</script>"#)
        .expect("svelte script regex")
});
static LANG_TS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\blang\s*=\s*["']ts["']"#).expect("lang ts regex"));
static COMPONENT_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<\s*([A-Z][A-Za-z0-9_.]*)\b"#).expect("component tag regex"));
static VUE_HANDLER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:@|v-on:)[A-Za-z0-9_:-]+\s*=\s*["']([A-Za-z_$][A-Za-z0-9_$]*)"#)
        .expect("vue handler regex")
});
static SVELTE_HANDLER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"on:[A-Za-z0-9_:-]+\s*=\s*\{\s*([A-Za-z_$][A-Za-z0-9_$]*)"#)
        .expect("svelte handler regex")
});

pub fn parse_sfc(
    jsts: &JsTsParser,
    file_path: &str,
    content: &str,
    language: Language,
    timeout_micros: Option<u64>,
) -> CcResult<ParseOutcome> {
    let script_re = match language {
        Language::Vue => &*VUE_SCRIPT_RE,
        Language::Svelte => &*SVELTE_SCRIPT_RE,
        _ => unreachable!("parse_sfc called for non-SFC language"),
    };

    let mut combined_script = String::new();
    let mut script_language = Language::JavaScript;
    let mut script_blocks = 0usize;
    for cap in script_re.captures_iter(content) {
        let Some(body) = cap.name("body") else {
            continue;
        };
        let attrs = cap.name("attrs").map(|m| m.as_str()).unwrap_or("");
        if LANG_TS_RE.is_match(attrs) {
            script_language = Language::TypeScript;
        }
        let start_line = content[..body.start()]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        combined_script.push_str(&"\n".repeat(start_line));
        combined_script.push_str(body.as_str());
        combined_script.push('\n');
        script_blocks += 1;
    }

    let mut outcome = if combined_script.trim().is_empty() {
        ParseOutcome::default()
    } else {
        jsts.parse_with_timeout(file_path, &combined_script, script_language, timeout_micros)?
    };

    let tier = ParserTier::Heuristic;
    let confidence = 0.78;
    let component_name = component_name_from_path(file_path);
    let total_lines = content.lines().count().max(1) as u32;
    let component_uid = StableId::symbol_uid(
        file_path,
        &component_name,
        SymbolKind::Component.as_str(),
        Some("component"),
    );
    let component_symbol = SymbolRecord {
        symbol_id: StableId::edge_id("sym", file_path, 1, 0),
        file_path: file_path.to_string(),
        name: component_name.clone(),
        kind: SymbolKind::Component,
        container: None,
        start_line: 1,
        end_line: total_lines,
        start_col: 0,
        end_col: 0,
        signature: Some(format!("{} component", language.as_str())),
        doc: None,
        parser_tier: tier,
        parser_confidence: confidence,
        qname: Some(component_name.clone()),
        parent_symbol_id: None,
        scope_id: None,
        export_name: Some(component_name.clone()),
        is_default_export: true,
        symbol_uid: Some(component_uid.clone()),
        framework_role: Some("component".to_string()),
        receiver_type: None,
        param_types: None,
        return_type: None,
        param_count: None,
        base_types: None,
        implements: None,
    };
    outcome.symbols.insert(0, component_symbol);

    add_template_refs(
        file_path,
        content,
        language,
        &component_name,
        &component_uid,
        &mut outcome,
    );

    outcome.summary = format!(
        "{} ({}, {} lines, {} script block(s), {} symbols)",
        file_path,
        language.as_str(),
        total_lines,
        script_blocks,
        outcome.symbols.len()
    );
    outcome.parser_tier = tier;
    outcome.parser_confidence = confidence;
    Ok(outcome)
}

fn component_name_from_path(file_path: &str) -> String {
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Component");
    let mut out = String::new();
    let mut upper = true;
    for ch in stem.chars() {
        if ch == '-' || ch == '_' || ch == ' ' {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "Component".to_string()
    } else {
        out
    }
}

fn line_col_at(content: &str, byte_offset: usize) -> (u32, u32) {
    let prefix = &content[..byte_offset.min(content.len())];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32 + 1;
    let col = prefix
        .rsplit('\n')
        .next()
        .map(|s| s.chars().count())
        .unwrap_or(0) as u32;
    (line, col)
}

fn add_template_refs(
    file_path: &str,
    content: &str,
    language: Language,
    component_name: &str,
    component_uid: &str,
    outcome: &mut ParseOutcome,
) {
    for cap in COMPONENT_TAG_RE.captures_iter(content) {
        let Some(name) = cap.get(1) else { continue };
        if name.as_str() == component_name {
            continue;
        }
        let (line, col) = line_col_at(content, name.start());
        outcome.symbol_refs.push(SymbolRefRecord {
            ref_id: StableId::ref_id(file_path, name.as_str(), line, col),
            file_path: file_path.to_string(),
            symbol_name: name.as_str().to_string(),
            container: Some(component_name.to_string()),
            ref_kind: "component_usage".to_string(),
            line,
            column: col,
            target_symbol_id: None,
            target_file_path: None,
            target_symbol_uid: None,
            ref_name: Some(name.as_str().to_string()),
            scope_id: None,
            resolution_kind: ResolutionKind::Unresolved,
            resolution_confidence: 0.0,
            resolution_strategy: "sfc_template_component".to_string(),
            ref_end_line: Some(line),
            ref_end_col: Some(col + name.as_str().len() as u32),
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.75,
        });
    }

    let handler_re = match language {
        Language::Vue => &*VUE_HANDLER_RE,
        Language::Svelte => &*SVELTE_HANDLER_RE,
        _ => return,
    };
    for cap in handler_re.captures_iter(content) {
        let Some(name) = cap.get(1) else { continue };
        let (line, col) = line_col_at(content, name.start());
        outcome.call_edges.push(CallEdgeRecord {
            edge_id: StableId::edge_id("call", file_path, line, col),
            file_path: file_path.to_string(),
            caller_symbol: Some(component_name.to_string()),
            callee_symbol: name.as_str().to_string(),
            line,
            start_col: col,
            end_line: Some(line),
            end_col: col + name.as_str().len() as u32,
            target_symbol_id: None,
            target_file_path: None,
            caller_symbol_id: None,
            callee_ref_id: None,
            caller_symbol_uid: Some(component_uid.to_string()),
            callee_symbol_uid: None,
            dispatch_kind: DispatchKind::EventEmitter,
            call_kind: "template_event".to_string(),
            resolution_kind: ResolutionKind::Unresolved,
            resolution_confidence: 0.0,
            resolution_strategy: "sfc_template_handler".to_string(),
            receiver_expr: None,
            arg_count: None,
            is_optional_chain: false,
            is_awaited: false,
            is_constructor: false,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.75,
            synthesized_by: Some("sfc_template".to_string()),
            synthesis_key: Some(format!("{}:{}", component_name, name.as_str())),
            registered_file: Some(file_path.to_string()),
            registered_line: Some(line),
        });
    }
}
