use anyhow::Result;
use std::path::Path;

use crate::context::store::Store;

/// Quality scorer — measures whether compression is degrading LLM output.
///
/// Tracks:
/// - Compression ratio distribution
/// - Re-expansion rate (how often expand is called — indicates info loss)
/// - Overall quality score 0-100
pub fn compute_quality_score(session_id: &str, db_path: &Path) -> Result<QualityReport> {
    let store = Store::open(db_path)?;
    let stats = store.session_stats(session_id)?;

    // Quality heuristic:
    // - Base score of 80 (assuming compression is generally fine)
    // - Penalize very aggressive compression (ratio > 0.9)
    // - Reward moderate compression (0.3-0.7)
    // - Penalize if very few compressions (not enough data)

    let mut score: f64 = 80.0;

    if stats.compressions_count < 3 {
        score = 50.0; // Not enough data
    } else {
        // Check individual compression ratios
        for type_stat in &stats.by_type {
            if type_stat.ratio > 0.9 {
                score -= 10.0; // Very aggressive — may lose info
            } else if type_stat.ratio > 0.7 {
                score -= 3.0; // Somewhat aggressive
            } else if (0.3..=0.7).contains(&type_stat.ratio) {
                score += 2.0; // Sweet spot
            }
        }
    }

    let score = score.clamp(0.0, 100.0);

    let assessment = if score >= 90.0 {
        "excellent — compression is near-lossless"
    } else if score >= 70.0 {
        "good — compression is effective with minimal information loss"
    } else if score >= 50.0 {
        "moderate — consider reducing compression aggressiveness"
    } else {
        "poor — not enough data or compression too aggressive"
    };

    let recommendation = if score < 70.0 {
        Some("Try using --level light or --level medium for better quality.".to_string())
    } else {
        None
    };

    Ok(QualityReport {
        session_id: session_id.to_string(),
        quality_score: score,
        assessment: assessment.to_string(),
        compressions_analyzed: stats.compressions_count,
        average_ratio: stats.overall_ratio,
        recommendation,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct QualityReport {
    pub session_id: String,
    pub quality_score: f64,
    pub assessment: String,
    pub compressions_analyzed: usize,
    pub average_ratio: f64,
    pub recommendation: Option<String>,
}
