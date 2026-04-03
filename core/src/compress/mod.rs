pub mod code;
pub mod conversation;
pub mod json;
pub mod mcp;
pub mod output;

use crate::ContentType;

/// Detect content type from raw text using heuristics.
pub fn detect_content_type(content: &str) -> ContentType {
    let trimmed = content.trim();

    // Empty or very short — not worth classifying
    if trimmed.len() < 10 {
        return ContentType::Unknown;
    }

    // MCP schema detection — JSON with "name" + "input_schema" keys
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if is_mcp_schema(&val) {
                return ContentType::McpSchema;
            }
            return ContentType::Json;
        }
    }

    // Command output detection — ANSI escapes, common command patterns
    if has_ansi_escapes(content)
        || looks_like_command_output(content)
    {
        return ContentType::CommandOutput;
    }

    // Code detection — try language heuristics
    if let Some(lang) = crate::utils::treesitter::detect_language_from_content(content) {
        return ContentType::Code { language: lang };
    }

    // Conversation detection — alternating user/assistant turns
    if looks_like_conversation(content) {
        return ContentType::Conversation;
    }

    ContentType::Unknown
}

fn has_ansi_escapes(content: &str) -> bool {
    content.contains("\x1b[")
}

fn looks_like_command_output(content: &str) -> bool {
    let first_lines: Vec<&str> = content.lines().take(5).collect();
    let indicators = [
        "warning:", "error:", "PASS", "FAIL", "ok ", "test ",
        "Compiling", "Downloading", "Installing", "running ",
        "✓", "✗", "●", "$", ">>>",
    ];
    first_lines.iter().any(|line| {
        indicators.iter().any(|ind| line.contains(ind))
    })
}

fn is_mcp_schema(val: &serde_json::Value) -> bool {
    // Single tool definition
    if val.get("name").is_some() && val.get("input_schema").is_some() {
        return true;
    }
    // Array of tool definitions
    if let Some(arr) = val.as_array() {
        return arr.iter().any(|item| {
            item.get("name").is_some() && item.get("input_schema").is_some()
        });
    }
    false
}

fn looks_like_conversation(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().take(20).collect();
    let turn_markers = ["User:", "Assistant:", "Human:", "AI:", "user:", "assistant:"];
    let matches = lines
        .iter()
        .filter(|line| turn_markers.iter().any(|m| line.starts_with(m)))
        .count();
    matches >= 2
}
