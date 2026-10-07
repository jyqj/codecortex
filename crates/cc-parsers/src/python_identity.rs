//! Opt-in AST assertions only. No symbol extraction or production identity wiring.
use cc_model::{
    declaration_identity::{content_digest, DeclarationInput, DeclarationSegment, ScopeKind},
    source::ByteSpan,
    CcError, CcResult,
};

/// Independent adapter budgets; these do not bound tree-sitter's peak allocation.
#[derive(Debug, Clone, Copy)]
pub struct PythonIdentityLimits {
    pub source_bytes: usize,
    pub visited_nodes: usize,
    pub tree_depth: usize,
    pub declarations: usize,
    pub output_segments: usize,
    /// Sum of cloned path, digest and ancestry-name bytes in returned inputs.
    pub output_text_bytes: usize,
    pub parse_timeout_micros: u64,
}
impl Default for PythonIdentityLimits {
    fn default() -> Self {
        Self {
            source_bytes: 1_048_576,
            visited_nodes: 200_000,
            tree_depth: 128,
            declarations: 4096,
            output_segments: 32_768,
            output_text_bytes: 8_388_608,
            parse_timeout_micros: 100_000,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonIdentityReason {
    InvalidUtf8,
    SyntaxError,
    UnsupportedAst,
    SourceLimit,
    WorkLimit,
    DepthLimit,
    OutputLimit,
}
#[derive(Debug, PartialEq, Eq)]
pub enum PythonIdentityOutcome {
    Inputs(Vec<DeclarationInput>),
    Unavailable(PythonIdentityReason),
}

/// Parse exactly the supplied original bytes with the existing Python grammar.
/// Any ERROR/MISSING rejects the whole file, including otherwise valid siblings.
/// A successful input is a syntactic declaration assertion, never importability.
/// Non-ASCII identifiers are preserved for the model's UnsupportedIdentifier policy.
/// Limits must all be finite/nonzero. No partial output survives a limit/error.
pub fn declaration_inputs(
    file_path: &str,
    source: &[u8],
    limits: PythonIdentityLimits,
) -> CcResult<PythonIdentityOutcome> {
    use PythonIdentityReason::*;
    let unavailable = |reason| Ok(PythonIdentityOutcome::Unavailable(reason));
    if [
        limits.source_bytes,
        limits.visited_nodes,
        limits.tree_depth,
        limits.declarations,
        limits.output_segments,
        limits.output_text_bytes,
    ]
    .contains(&0)
        || limits.parse_timeout_micros == 0
        || file_path.len() > 4096
    {
        return Err(CcError::InvalidParams(
            "invalid Python identity limits/path".into(),
        ));
    }
    if source.len() > limits.source_bytes {
        return unavailable(SourceLimit);
    }
    let Ok(content) = std::str::from_utf8(source) else {
        return unavailable(InvalidUtf8);
    };
    let tree = crate::parse_common::parse_tree(
        &tree_sitter_python::LANGUAGE.into(),
        content,
        file_path,
        Some(limits.parse_timeout_micros),
    )?;
    let root = tree.root_node();
    if root.has_error() {
        return unavailable(SyntaxError);
    }
    // The pinned grammar can start modules after leading trivia/BOM, including
    // an empty module at source.len(). The parser still consumed original bytes.
    if root.kind() != "module"
        || root.start_byte() > root.end_byte()
        || root.end_byte() > source.len()
    {
        return unavailable(UnsupportedAst);
    }
    let digest = content_digest(source);
    let mut output = Vec::new();
    let mut ancestry = Vec::new();
    let mut cursor = tree.walk();
    let mut depth = 1usize;
    let mut visited = 0usize;
    let mut segments = 0usize;
    let mut ancestry_text = 0usize;
    let mut output_text = 0usize;
    loop {
        visited += 1;
        if visited > limits.visited_nodes {
            return unavailable(WorkLimit);
        }
        if depth > limits.tree_depth {
            return unavailable(DepthLimit);
        }
        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            return unavailable(SyntaxError);
        }
        if let Some(kind) = scope_kind(node) {
            let Some(segment) = segment(node, source, kind) else {
                return unavailable(UnsupportedAst);
            };
            if output.len() >= limits.declarations
                || ancestry.len() + 1 > limits.output_segments.saturating_sub(segments)
            {
                return unavailable(OutputLimit);
            }
            let next_ancestry_text = ancestry_text.checked_add(segment.name.len());
            let next_output_text = next_ancestry_text
                .and_then(|n| n.checked_add(file_path.len()))
                .and_then(|n| n.checked_add(digest.len()))
                .and_then(|n| n.checked_add(output_text));
            let Some(next_output_text) =
                next_output_text.filter(|n| *n <= limits.output_text_bytes)
            else {
                return unavailable(OutputLimit);
            };
            ancestry_text = next_ancestry_text.unwrap();
            output_text = next_output_text;
            ancestry.push(segment);
            segments += ancestry.len();
            output.push(DeclarationInput {
                file_path: file_path.into(),
                source_digest: digest.clone(),
                ancestry: ancestry.clone(),
            });
        }
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            if scope_kind(cursor.node()).is_some() {
                ancestry_text -= ancestry.pop().expect("entered declaration").name.len();
            }
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(PythonIdentityOutcome::Inputs(output));
            }
            depth -= 1;
        }
    }
}
fn scope_kind(node: tree_sitter::Node<'_>) -> Option<ScopeKind> {
    match node.kind() {
        "class_definition" => Some(ScopeKind::Class),
        "function_definition" => Some(ScopeKind::Function),
        _ => None,
    }
}
fn segment(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    kind: ScopeKind,
) -> Option<DeclarationSegment> {
    let name = node.child_by_field_name("name")?;
    let body = node.child_by_field_name("body")?;
    if name.kind() != "identifier"
        || body.kind() != "block"
        || body.start_byte() == body.end_byte()
        || body.named_child_count() == 0
    {
        return None;
    }
    if kind == ScopeKind::Function && node.child_by_field_name("parameters")?.kind() != "parameters"
    {
        return None;
    }
    let canonical = match node.parent() {
        Some(parent) if parent.kind() == "decorated_definition" => {
            if parent.child_by_field_name("definition")? != node {
                return None;
            }
            parent
        }
        _ => node,
    };
    let span = ByteSpan {
        start: canonical.start_byte(),
        end: canonical.end_byte(),
    };
    let name_span = ByteSpan {
        start: name.start_byte(),
        end: name.end_byte(),
    };
    if span.is_empty() || !span.contains(name_span) || span.end > source.len() {
        return None;
    }
    let text = std::str::from_utf8(source.get(name_span.start..name_span.end)?).ok()?;
    // The grammar can label hard keywords as identifier fields in recovery-free trees.
    if [
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "try", "while", "with", "yield",
    ]
    .contains(&text)
    {
        return None;
    }
    Some(DeclarationSegment {
        name: text.into(),
        kind,
        span,
        name_span,
    })
}
