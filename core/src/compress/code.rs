use crate::{CompressionLevel, Language};

/// AST-aware code compression.
///
/// Without compiled tree-sitter grammars, falls back to heuristic-based
/// folding that works across languages using regex patterns for common
/// constructs (function definitions, class definitions, impl blocks).
///
/// Fold levels:
/// - Full: keep entire body
/// - Summary: signature + docstring + first/last lines
/// - SignatureOnly: just the signature with line count
/// - Omitted: removed entirely
pub fn compress_code(content: &str, language: Language, level: CompressionLevel) -> String {
    // Try AST-based compression first
    if let Some(result) = try_ast_compress(content, language, level) {
        return result;
    }

    // Fallback: heuristic-based compression
    heuristic_compress(content, language, level)
}

/// AST-based compression using tree-sitter (surgical, byte-accurate).
///
/// Traverses the CST and folds function/method bodies at exact node boundaries.
/// Falls back to heuristic if no grammar is available.
fn try_ast_compress(content: &str, language: Language, level: CompressionLevel) -> Option<String> {
    let tree = crate::utils::treesitter::parse(content, language).ok()??;
    let root = tree.root_node();
    let source_bytes = content.as_bytes();

    // Language-specific node types
    let (fn_kind, body_field): (&str, &str) = match language {
        Language::Rust       => ("function_item",       "body"),
        Language::Python     => ("function_definition", "body"),
        Language::JavaScript => ("function_declaration","body"),
        Language::TypeScript => ("function_declaration","body"),
        _ => return None,
    };

    // Collect (start_byte, end_byte, replacement) for each foldable body.
    // We work on bytes so edits stay correct for multi-byte UTF-8.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    collect_fn_edits(&root, fn_kind, body_field, source_bytes, level, &mut edits);

    if edits.is_empty() {
        return None; // Nothing to fold — let heuristic handle it
    }

    // Apply edits in reverse byte order so earlier offsets stay valid
    edits.sort_by(|a, b| b.0.cmp(&a.0));

    let mut result = content.as_bytes().to_vec();
    for (start, end, replacement) in edits {
        result.splice(start..end, replacement.into_bytes());
    }

    String::from_utf8(result).ok()
}

/// Recursively collect fold edits for all function nodes in the CST.
fn collect_fn_edits(
    node: &tree_sitter::Node,
    fn_kind: &str,
    body_field: &str,
    source: &[u8],
    level: CompressionLevel,
    edits: &mut Vec<(usize, usize, String)>,
) {
    if node.kind() == fn_kind {
        if let Some(body) = node.child_by_field_name(body_field) {
            let body_text = &source[body.start_byte()..body.end_byte()];
            let body_lines = body_text.iter().filter(|&&b| b == b'\n').count() + 1;

            let fold = fold_decision(body_lines, level);
            match fold {
                FoldAction::Full => {} // keep as-is
                FoldAction::Summary => {
                    // Keep first 2 + last 1 lines of body, fold the middle
                    let body_str = std::str::from_utf8(body_text).unwrap_or("");
                    let lines: Vec<&str> = body_str.lines().collect();
                    if lines.len() > 6 {
                        let kept_head: Vec<&str> = lines.iter().take(3).cloned().collect();
                        let kept_tail: Vec<&str> = lines.iter().rev().take(2).cloned().collect();
                        let folded = lines.len() - 5;
                        let replacement = format!(
                            "{}\n    // ... {folded} lines folded ...\n{}",
                            kept_head.join("\n"),
                            kept_tail.into_iter().rev().collect::<Vec<_>>().join("\n")
                        );
                        edits.push((body.start_byte(), body.end_byte(), replacement));
                    }
                }
                FoldAction::SignatureOnly => {
                    // Fold entire body to one line with count
                    let replacement = format!("{{ /* ... {body_lines} lines */ }}");
                    edits.push((body.start_byte(), body.end_byte(), replacement));
                }
            }
        }
        // Don't recurse into nested functions — we handle top-level only
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_fn_edits(&child, fn_kind, body_field, source, level, edits);
    }
}

/// Heuristic-based code compression using regex/pattern matching.
/// Works without tree-sitter by recognizing common code patterns.
fn heuristic_compress(content: &str, language: Language, level: CompressionLevel) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // For small files, don't compress
    let min_lines = match level {
        CompressionLevel::Light => 100,
        CompressionLevel::Medium => 50,
        CompressionLevel::Aggressive => 20,
    };

    if total_lines <= min_lines {
        return content.to_string();
    }

    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Always keep: imports, use statements, module declarations
        if is_import(line, language) || is_type_definition(line, language) {
            result.push(line.to_string());
            i += 1;
            continue;
        }

        // Detect function/method definitions
        if is_function_start(line, language) {
            let (_sig, body_start, body_end) = find_block_extent(&lines, i, language);

            if body_end > body_start {
                let body_lines = body_end - body_start;
                let fold = fold_decision(body_lines, level);

                match fold {
                    FoldAction::Full => {
                        for j in i..=body_end.min(lines.len() - 1) {
                            result.push(lines[j].to_string());
                        }
                    }
                    FoldAction::Summary => {
                        // Signature + first 2 lines + last line + fold marker
                        result.push(lines[i].to_string());
                        let content_start = body_start + 1;
                        let content_end = body_end.saturating_sub(1);
                        if content_start < lines.len() {
                            result.push(lines[content_start].to_string());
                        }
                        if content_start + 1 < content_end && content_start + 1 < lines.len() {
                            result.push(lines[content_start + 1].to_string());
                        }
                        if body_lines > 4 {
                            result.push(format!(
                                "    // ... {body_lines} lines folded ..."
                            ));
                        }
                        if content_end < lines.len() && content_end > content_start + 2 {
                            result.push(lines[content_end].to_string());
                        }
                        if body_end < lines.len() {
                            result.push(lines[body_end].to_string());
                        }
                    }
                    FoldAction::SignatureOnly => {
                        result.push(format!(
                            "{} {{ /* ... {body_lines} lines */ }}",
                            lines[i].trim_end().trim_end_matches('{').trim_end()
                        ));
                    }
                }

                i = body_end + 1;
                continue;
            }
        }

        result.push(line.to_string());
        i += 1;
    }

    result.join("\n")
}

enum FoldAction {
    Full,
    Summary,
    SignatureOnly,
}

fn fold_decision(body_lines: usize, level: CompressionLevel) -> FoldAction {
    match level {
        CompressionLevel::Light => {
            if body_lines > 50 {
                FoldAction::Summary
            } else {
                FoldAction::Full
            }
        }
        CompressionLevel::Medium => {
            if body_lines > 30 {
                FoldAction::SignatureOnly
            } else if body_lines > 10 {
                FoldAction::Summary
            } else {
                FoldAction::Full
            }
        }
        CompressionLevel::Aggressive => {
            if body_lines > 10 {
                FoldAction::SignatureOnly
            } else if body_lines > 5 {
                FoldAction::Summary
            } else {
                FoldAction::Full
            }
        }
    }
}

fn is_import(line: &str, lang: Language) -> bool {
    let trimmed = line.trim();
    match lang {
        Language::Rust => trimmed.starts_with("use ") || trimmed.starts_with("mod "),
        Language::Python => trimmed.starts_with("import ") || trimmed.starts_with("from "),
        Language::Go => trimmed.starts_with("import "),
        Language::TypeScript | Language::JavaScript => {
            trimmed.starts_with("import ") || trimmed.starts_with("require(")
                || trimmed.starts_with("const ") && trimmed.contains("require(")
        }
        Language::Java => trimmed.starts_with("import ") || trimmed.starts_with("package "),
        Language::C | Language::Cpp => trimmed.starts_with("#include"),
        Language::Ruby => trimmed.starts_with("require ") || trimmed.starts_with("require_relative "),
        Language::Php => trimmed.starts_with("use ") || trimmed.starts_with("require ") || trimmed.starts_with("include "),
        _ => false,
    }
}

fn is_type_definition(line: &str, lang: Language) -> bool {
    let trimmed = line.trim();
    match lang {
        Language::Rust => {
            trimmed.starts_with("pub struct ") || trimmed.starts_with("struct ")
                || trimmed.starts_with("pub enum ") || trimmed.starts_with("enum ")
                || trimmed.starts_with("pub trait ") || trimmed.starts_with("trait ")
                || trimmed.starts_with("pub type ") || trimmed.starts_with("type ")
        }
        Language::TypeScript | Language::JavaScript => {
            trimmed.starts_with("interface ") || trimmed.starts_with("type ")
                || trimmed.starts_with("export interface ") || trimmed.starts_with("export type ")
        }
        Language::Python => trimmed.starts_with("class "),
        Language::Go => trimmed.starts_with("type "),
        Language::Java => {
            (trimmed.contains("class ") || trimmed.contains("interface ") || trimmed.contains("enum "))
                && !trimmed.starts_with("//")
        }
        _ => false,
    }
}

fn is_function_start(line: &str, lang: Language) -> bool {
    let trimmed = line.trim();
    match lang {
        Language::Rust => {
            (trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ")
                || trimmed.starts_with("pub async fn ") || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub(crate) fn "))
                && !trimmed.starts_with("//")
        }
        Language::TypeScript | Language::JavaScript => {
            (trimmed.starts_with("function ") || trimmed.starts_with("export function ")
                || trimmed.starts_with("async function ") || trimmed.starts_with("export async function ")
                || (trimmed.contains("(") && trimmed.contains(") {") && !trimmed.starts_with("if ") && !trimmed.starts_with("for ") && !trimmed.starts_with("while ")))
                && !trimmed.starts_with("//")
        }
        Language::Python => {
            (trimmed.starts_with("def ") || trimmed.starts_with("async def "))
                && trimmed.contains("(")
                && !trimmed.starts_with("#")
        }
        Language::Go => {
            trimmed.starts_with("func ") && !trimmed.starts_with("//")
        }
        Language::Java => {
            trimmed.contains("(") && trimmed.ends_with("{")
                && (trimmed.contains("public ") || trimmed.contains("private ") || trimmed.contains("protected "))
                && !trimmed.starts_with("//") && !trimmed.starts_with("if ") && !trimmed.starts_with("for ")
        }
        Language::C | Language::Cpp => {
            trimmed.contains("(") && trimmed.ends_with("{")
                && !trimmed.starts_with("#") && !trimmed.starts_with("//")
                && !trimmed.starts_with("if ") && !trimmed.starts_with("for ") && !trimmed.starts_with("while ")
        }
        _ => false,
    }
}

/// Find the extent of a brace-delimited block starting at line `start`.
/// Returns (signature_line, body_start, body_end).
fn find_block_extent(lines: &[&str], start: usize, lang: Language) -> (usize, usize, usize) {
    // For Python: use indentation
    if lang == Language::Python {
        return find_python_block(lines, start);
    }

    // For brace-delimited languages
    let mut depth = 0i32;
    let mut body_start = start;
    let mut found_open = false;

    for i in start..lines.len() {
        for ch in lines[i].chars() {
            if ch == '{' {
                if !found_open {
                    body_start = i;
                    found_open = true;
                }
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 && found_open {
                    return (start, body_start, i);
                }
            }
        }
    }

    // No matching close found — return to end of file
    (start, body_start, lines.len().saturating_sub(1))
}

fn find_python_block(lines: &[&str], start: usize) -> (usize, usize, usize) {
    let base_indent = lines[start].len() - lines[start].trim_start().len();
    let body_start = start + 1;

    for i in body_start..lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= base_indent {
            return (start, body_start, i.saturating_sub(1));
        }
    }

    (start, body_start, lines.len().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_files_not_compressed() {
        let code = "fn main() {\n    println!(\"hello\");\n}\n";
        let result = compress_code(code, Language::Rust, CompressionLevel::Medium);
        assert_eq!(result, code);
    }

    #[test]
    fn large_function_gets_folded() {
        let mut lines = vec!["fn big_function() {".to_string()];
        for i in 0..50 {
            lines.push(format!("    let x{i} = {i};"));
        }
        lines.push("}".to_string());
        let code = lines.join("\n");

        let result = compress_code(&code, Language::Rust, CompressionLevel::Aggressive);
        assert!(result.len() < code.len());
        assert!(result.contains("lines"));
    }
}
