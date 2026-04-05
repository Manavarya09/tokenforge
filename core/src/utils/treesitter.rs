use crate::Language;
use anyhow::{Context, Result};

/// Get the compiled tree-sitter language grammar for a given language.
/// Returns None for languages without linked grammars.
pub fn get_language(lang: Language) -> Option<tree_sitter::Language> {
    match lang {
        Language::Rust       => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::Python     => Some(tree_sitter_python::LANGUAGE.into()),
        Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        // Additional grammars can be linked here as needed
        _ => None,
    }
}

/// Parse source code into a CST using tree-sitter.
/// Returns Ok(None) if no grammar is available for the language.
pub fn parse(source: &str, lang: Language) -> Result<Option<tree_sitter::Tree>> {
    let Some(language) = get_language(lang) else {
        return Ok(None);
    };

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .context("failed to set parser language")?;

    Ok(parser.parse(source, None))
}

/// Detect language from file path extension.
pub fn detect_language_from_path(path: &str) -> Option<Language> {
    let ext = path.rsplit('.').next()?;
    Language::from_extension(ext)
}

/// Detect language from content heuristics (shebang, keywords).
pub fn detect_language_from_content(content: &str) -> Option<Language> {
    let first_line = content.lines().next().unwrap_or("");

    // Check shebang
    if first_line.starts_with("#!") {
        if first_line.contains("python") {
            return Some(Language::Python);
        }
        if first_line.contains("node") || first_line.contains("deno") || first_line.contains("bun")
        {
            return Some(Language::JavaScript);
        }
        if first_line.contains("bash") || first_line.contains("sh") || first_line.contains("zsh") {
            return Some(Language::Bash);
        }
        if first_line.contains("ruby") {
            return Some(Language::Ruby);
        }
        if first_line.contains("php") {
            return Some(Language::Php);
        }
    }

    // Check distinctive keywords/patterns
    if content.contains("fn main()") || content.contains("pub fn ") || content.contains("impl ") {
        return Some(Language::Rust);
    }
    if content.contains("func ") && content.contains("package ") {
        return Some(Language::Go);
    }
    if content.contains("def ") && content.contains("import ") && !content.contains("{") {
        return Some(Language::Python);
    }
    if content.contains("interface ") && content.contains(": ") && content.contains("export ") {
        return Some(Language::TypeScript);
    }
    if content.contains("function ") || content.contains("const ") || content.contains("=> {") {
        return Some(Language::JavaScript);
    }
    if content.contains("public class ") || content.contains("public static void main") {
        return Some(Language::Java);
    }

    None
}
