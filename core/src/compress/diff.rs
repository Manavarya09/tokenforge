//! Semantic diff compression — when new content is highly similar to
//! previously stored content, store only the diff instead of the full text.
//!
//! Savings example: if tool output changes by 10% between calls, we store
//! ~10% of the tokens instead of 100%. On expand, we reconstruct by
//! applying the patch to the base.

use crate::context::store::Store;
use anyhow::Result;
use std::collections::HashSet;

/// Marker prefix used in compressed content to signal diff-encoding.
/// Format: `TOKENFORGE_DIFF:<base_hash>:<unified_diff>`
pub const DIFF_MARKER: &str = "TOKENFORGE_DIFF:";

/// Result of a successful diff compression.
pub struct DiffCompressResult {
    pub base_hash: String,
    pub diff_text: String,
    pub diff_tokens: usize,
}

/// Try to diff-compress `content` against a recently stored similar item.
///
/// Returns Some(result) if a similar base was found AND the diff is
/// meaningfully smaller (>30% savings). Returns None otherwise.
pub fn try_diff_compress(
    content: &str,
    store: &Store,
    session_id: &str,
) -> Option<DiffCompressResult> {
    // Get candidate base items from the session
    let candidates = store.recent_compressed_items(session_id, 20).ok()?;

    let mut best: Option<(String, String, f64)> = None; // (base_hash, diff, ratio)

    for (base_hash, base_content) in candidates {
        let sim = line_jaccard(&base_content, content);
        if sim < 0.60 {
            continue; // not similar enough to be worth diffing
        }

        let patch = diffy::create_patch(&base_content, content);
        let diff_text = patch.to_string();

        // Only use diff if it's meaningfully shorter
        let savings = 1.0 - (diff_text.len() as f64 / content.len() as f64);
        if savings > 0.30 {
            // Pick the best (most savings)
            if best.as_ref().map_or(true, |(_, _, s)| savings > *s) {
                best = Some((base_hash, diff_text, savings));
            }
        }
    }

    best.map(|(base_hash, diff_text, _)| {
        let diff_tokens = crate::utils::tokens::estimate_tokens_fast(&diff_text);
        DiffCompressResult { base_hash, diff_text, diff_tokens }
    })
}

/// Reconstruct original content from a diff record.
pub fn reconstruct(base_hash: &str, diff_text: &str, store: &Store) -> Result<String> {
    let base_content = store.get_original(base_hash)?;
    let patch = diffy::Patch::from_str(diff_text)
        .map_err(|e| anyhow::anyhow!("invalid patch: {e}"))?;
    diffy::apply(&base_content, &patch)
        .map_err(|e| anyhow::anyhow!("patch apply failed: {e}"))
}

/// Check if a string starts with the diff marker.
pub fn is_diff_encoded(content: &str) -> bool {
    content.starts_with(DIFF_MARKER)
}

/// Parse a diff-encoded string into (base_hash, diff_text).
pub fn parse_diff_encoded(encoded: &str) -> Option<(String, String)> {
    let rest = encoded.strip_prefix(DIFF_MARKER)?;
    // base_hash is 64 hex chars (blake3)
    if rest.len() < 65 {
        return None;
    }
    let (hash_part, diff_part) = rest.split_at(64);
    let diff_part = diff_part.strip_prefix(':').unwrap_or(diff_part);
    Some((hash_part.to_string(), diff_part.to_string()))
}

/// Encode a diff result into the canonical compressed string.
pub fn encode_diff(base_hash: &str, diff_text: &str) -> String {
    format!("{DIFF_MARKER}{base_hash}:{diff_text}")
}

/// Line-level Jaccard similarity: |A ∩ B| / |A ∪ B| where A and B are
/// sets of trimmed non-empty lines. O(n) with a HashSet.
fn line_jaccard(a: &str, b: &str) -> f64 {
    let set_a: HashSet<&str> = a.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let set_b: HashSet<&str> = b.lines().map(str::trim).filter(|l| !l.is_empty()).collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }

    let intersection = set_a.intersection(&set_b).count() as f64;
    let union = set_a.union(&set_b).count() as f64;
    intersection / union
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_identical() {
        assert_eq!(line_jaccard("a\nb\nc", "a\nb\nc"), 1.0);
    }

    #[test]
    fn jaccard_disjoint() {
        assert_eq!(line_jaccard("a\nb", "c\nd"), 0.0);
    }

    #[test]
    fn jaccard_partial() {
        let sim = line_jaccard("a\nb\nc\nd", "a\nb\ne\nf");
        assert!(sim > 0.2 && sim < 0.6);
    }

    #[test]
    fn roundtrip_encode_parse() {
        let hash = "a".repeat(64);
        let diff = "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n";
        let encoded = encode_diff(&hash, diff);
        let (parsed_hash, parsed_diff) = parse_diff_encoded(&encoded).unwrap();
        assert_eq!(parsed_hash, hash);
        assert_eq!(parsed_diff, diff);
    }

    #[test]
    fn is_diff_encoded_true() {
        let s = format!("{}{}", DIFF_MARKER, "x".repeat(64));
        assert!(is_diff_encoded(&s));
    }

    #[test]
    fn is_diff_encoded_false() {
        assert!(!is_diff_encoded("fn main() {}"));
    }
}
