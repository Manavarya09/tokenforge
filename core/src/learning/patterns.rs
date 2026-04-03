use anyhow::Result;
use std::path::Path;

use crate::context::store::Store;

/// Analyze token usage patterns for a session.
pub fn analyze_session(session_id: &str, db_path: &Path) -> Result<PatternAnalysis> {
    let store = Store::open(db_path)?;
    let stats = store.session_stats(session_id)?;

    // Determine session type from content type distribution
    let session_type = classify_session(&stats.by_type);

    // Find biggest token consumers
    let mut by_type = stats.by_type.clone();
    by_type.sort_by(|a, b| b.original_tokens.cmp(&a.original_tokens));

    let recommendations = generate_recommendations(&by_type, &session_type);

    Ok(PatternAnalysis {
        session_id: session_id.to_string(),
        session_type,
        total_tokens_processed: stats.total_original_tokens,
        total_tokens_saved: stats.tokens_saved,
        savings_percentage: stats.overall_ratio * 100.0,
        top_consumers: by_type,
        recommendations,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct PatternAnalysis {
    pub session_id: String,
    pub session_type: String,
    pub total_tokens_processed: usize,
    pub total_tokens_saved: usize,
    pub savings_percentage: f64,
    pub top_consumers: Vec<crate::TypeStats>,
    pub recommendations: Vec<String>,
}

fn classify_session(by_type: &[crate::TypeStats]) -> String {
    let code_tokens: usize = by_type
        .iter()
        .filter(|t| t.content_type.starts_with("code"))
        .map(|t| t.original_tokens)
        .sum();
    let output_tokens: usize = by_type
        .iter()
        .filter(|t| t.content_type == "command_output")
        .map(|t| t.original_tokens)
        .sum();
    let total: usize = by_type.iter().map(|t| t.original_tokens).sum();

    if total == 0 {
        return "empty".to_string();
    }

    let code_ratio = code_tokens as f64 / total as f64;
    let output_ratio = output_tokens as f64 / total as f64;

    if output_ratio > 0.5 {
        "debugging".to_string()
    } else if code_ratio > 0.5 {
        "code_review".to_string()
    } else {
        "feature_build".to_string()
    }
}

fn generate_recommendations(
    by_type: &[crate::TypeStats],
    session_type: &str,
) -> Vec<String> {
    let mut recs = Vec::new();

    for stat in by_type {
        if stat.ratio < 0.3 && stat.original_tokens > 1000 {
            recs.push(format!(
                "Consider increasing compression for '{}' — currently only {:.0}% savings on {} tokens",
                stat.content_type,
                stat.ratio * 100.0,
                stat.original_tokens,
            ));
        }
    }

    match session_type {
        "debugging" => {
            recs.push("Debugging session detected — tool output is the biggest consumer. Use --level aggressive for command output.".to_string());
        }
        "code_review" => {
            recs.push("Code review session — consider using aggressive code folding for files you've already reviewed.".to_string());
        }
        _ => {}
    }

    recs
}
