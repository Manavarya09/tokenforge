//! Auto-configure TokenForge as a Claude Code PostToolUse hook.
//!
//! Reads ~/.claude/settings.json, injects the hook entry idempotently,
//! and writes back atomically. Safe to run multiple times.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct SetupReport {
    pub settings_path: String,
    pub already_configured: bool,
    pub hook_command: String,
    pub dry_run: bool,
    pub message: String,
}

/// Find the tokenforge binary in common locations.
pub fn find_tokenforge_binary() -> Option<PathBuf> {
    // 1. Same binary currently running
    if let Ok(exe) = std::env::current_exe() {
        return Some(exe);
    }
    // 2. In $PATH
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let candidate = Path::new(dir).join("tokenforge");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // 3. ~/.cargo/bin
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home).join(".cargo/bin/tokenforge");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Get the Claude Code settings.json path.
fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".claude/settings.json")
}

/// Read settings.json, or return a default empty object.
fn read_settings(path: &Path) -> serde_json::Value {
    if let Ok(data) = std::fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    }
}

/// Check if a tokenforge hook is already configured.
fn already_configured(settings: &serde_json::Value) -> bool {
    let Some(hooks) = settings.get("hooks") else { return false };
    let Some(post_tool_use) = hooks.get("PostToolUse") else { return false };
    let Some(arr) = post_tool_use.as_array() else { return false };
    for entry in arr {
        // New format: { "matcher": "*", "hooks": [{ "type": "command", "command": "..." }] }
        if let Some(inner_hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
            for h in inner_hooks {
                if let Some(cmd) = h.get("command").and_then(|c| c.as_str()) {
                    if cmd.contains("tokenforge") {
                        return true;
                    }
                }
            }
        }
        // Legacy format: { "command": "..." }
        if let Some(cmd) = entry.get("command").and_then(|c| c.as_str()) {
            if cmd.contains("tokenforge") {
                return true;
            }
        }
    }
    false
}

/// Inject the PostToolUse hook entry into settings (mutates in place).
fn inject_hook(settings: &mut serde_json::Value, binary_path: &str) {
    let hook_command = format!(
        "{binary_path} hook --session ${{CLAUDE_SESSION_ID:-default}}"
    );
    let hook_entry = serde_json::json!({
        "matcher": "*",
        "hooks": [{ "type": "command", "command": hook_command }]
    });

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}));

    let ptu = hooks
        .as_object_mut()
        .unwrap()
        .entry("PostToolUse")
        .or_insert(serde_json::json!([]));

    if let Some(arr) = ptu.as_array_mut() {
        arr.push(hook_entry);
    }
}

/// Write settings atomically: write to tmp, then rename.
fn write_settings_atomic(path: &Path, settings: &serde_json::Value) -> Result<()> {
    let parent = path.parent().context("settings path has no parent")?;
    std::fs::create_dir_all(parent)?;

    let tmp_path = path.with_extension("json.tmp");
    let pretty = serde_json::to_string_pretty(settings)?;
    std::fs::write(&tmp_path, pretty)?;
    std::fs::rename(&tmp_path, path).context("atomic rename failed")?;
    Ok(())
}

/// Run setup — inject hook into ~/.claude/settings.json.
pub fn run_setup(dry_run: bool) -> Result<SetupReport> {
    let path = settings_path();
    let binary = find_tokenforge_binary()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "tokenforge".to_string());

    let hook_command = format!("{binary} hook --session ${{CLAUDE_SESSION_ID:-default}}");

    let mut settings = read_settings(&path);

    if already_configured(&settings) {
        return Ok(SetupReport {
            settings_path: path.to_string_lossy().to_string(),
            already_configured: true,
            hook_command,
            dry_run,
            message: "TokenForge hook already configured — nothing to do.".to_string(),
        });
    }

    if !dry_run {
        inject_hook(&mut settings, &binary);
        write_settings_atomic(&path, &settings)?;
    }

    Ok(SetupReport {
        settings_path: path.to_string_lossy().to_string(),
        already_configured: false,
        hook_command,
        dry_run,
        message: if dry_run {
            format!(
                "[DRY RUN] Would add PostToolUse hook to {}",
                path.display()
            )
        } else {
            format!(
                "PostToolUse hook added to {}. Restart Claude Code to activate.",
                path.display()
            )
        },
    })
}
