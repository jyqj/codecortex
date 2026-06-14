//! Shared tree-sitter parsing helpers used across language parsers.

use cc_model::{CcError, CcResult, Language};

/// Build a tree-sitter parser for `language`, apply an optional timeout, and
/// parse `content` into a syntax tree.
pub(crate) fn parse_tree(
    language: &tree_sitter::Language,
    content: &str,
    file_path: &str,
    timeout_micros: Option<u64>,
) -> CcResult<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(language).map_err(|e| CcError::Parse {
        file: file_path.to_string(),
        message: e.to_string(),
    })?;
    if let Some(timeout) = timeout_micros {
        parser.set_timeout_micros(timeout);
    }

    parser.parse(content, None).ok_or_else(|| CcError::Parse {
        file: file_path.to_string(),
        message: if timeout_micros.is_some() {
            "tree-sitter parse failed or timed out".to_string()
        } else {
            "tree-sitter parse failed".to_string()
        },
    })
}

/// 判断 `file_path` 是否像测试文件，按 `language` 分发到各语言既有的路径启发。
///
/// 这是 6 个 tree-sitter parser 的 `parse_with_timeout` 末尾原本各写一份的
/// `is_test` 判定的单一来源：每条分支逐字搬自原 parser，行为保持不变。
/// 未知/非测试语言返回 `false`（与现状一致——只有这 6 个 parser 产出
/// `is_test_file`，其他语言不进入此路径）。
pub(crate) fn is_test_file(file_path: &str, language: Language) -> bool {
    match language {
        Language::Rust => file_path.contains("/tests/") || file_path.ends_with("_test.rs"),
        Language::Python => {
            file_path.contains("/tests/")
                || file_path.contains("test_")
                || file_path.contains("_test.py")
                || file_path.ends_with("tests.py")
        }
        // JS/TS 及其方言共享同一套测试文件约定（.test./.spec./__tests__）。
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => {
            file_path.contains(".test.")
                || file_path.contains(".spec.")
                || file_path.contains("__tests__")
        }
        Language::Go => file_path.ends_with("_test.go"),
        Language::Java => {
            // 保留原行为：基于文件名（而非完整路径）的前/后缀判定。
            let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
            file_name.starts_with("Test") || file_name.ends_with("Test.java")
        }
        // C/Cpp 共享同一套判定，且原实现做了 to_lowercase。
        Language::C | Language::Cpp => {
            let lower = file_path.to_lowercase();
            lower.ends_with("_test.c")
                || lower.ends_with("_test.cpp")
                || lower.ends_with("_test.cc")
                || lower.ends_with("_test.cxx")
                || lower.contains("/test_")
                || lower.contains("/tests/")
                || lower.contains("_tests.c")
                || lower.contains("_tests.cpp")
        }
        // 兜底：仅上述 6 个 tree-sitter parser 的路径启发在此分发；其余语言
        //（含 spec_driven/generic）不应进入此路径，统一返回 false（与现状一致）。
        _ => false,
    }
}
