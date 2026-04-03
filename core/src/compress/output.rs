use crate::CompressionLevel;
use regex::Regex;
use std::sync::OnceLock;

static ANSI_RE: OnceLock<Regex> = OnceLock::new();

fn ansi_regex() -> &'static Regex {
    ANSI_RE.get_or_init(|| {
        Regex::new(r"\x1b\[[0-9;]*[A-Za-z]|\x1b\].*?\x07").unwrap()
    })
}

/// Compress command output (Bash, Grep, build logs, test output).
///
/// Strategies:
/// - Strip ANSI escape codes
/// - Deduplicate consecutive similar lines
/// - Summarize test results (pass/fail counts + failures only)
/// - Truncate to max lines with count annotation
/// - Preserve stderr-like content (errors, warnings)
pub fn compress_output(content: &str, level: CompressionLevel) -> String {
    let max_lines = match level {
        CompressionLevel::Light => 200,
        CompressionLevel::Medium => 80,
        CompressionLevel::Aggressive => 30,
    };

    let dedup_threshold = match level {
        CompressionLevel::Light => 1.0,   // exact match only
        CompressionLevel::Medium => 0.9,
        CompressionLevel::Aggressive => 0.8,
    };

    // Step 1: Strip ANSI
    let cleaned = strip_ansi(content);

    // Step 2: Try pattern-specific compression
    if let Some(test_summary) = try_compress_test_output(&cleaned, level) {
        return test_summary;
    }
    if let Some(compiler_summary) = try_compress_compiler_output(&cleaned, level) {
        return compiler_summary;
    }

    // Step 3: General dedup + truncate
    let lines: Vec<&str> = cleaned.lines().collect();
    let deduped = dedup_lines(&lines, dedup_threshold);

    // Step 4: Truncate
    if deduped.len() <= max_lines {
        return deduped.join("\n");
    }

    let mut result: Vec<String> = deduped[..max_lines].to_vec();
    let remaining = deduped.len() - max_lines;
    result.push(format!("[... {remaining} more lines truncated]"));

    result.join("\n")
}

fn strip_ansi(content: &str) -> String {
    ansi_regex().replace_all(content, "").to_string()
}

fn dedup_lines(lines: &[&str], threshold: f64) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut run_count = 0usize;
    let mut last_line: Option<&str> = None;

    for line in lines {
        let line = line.trim_end();
        if line.is_empty() {
            if run_count > 0 {
                flush_run(&mut result, last_line.unwrap_or(""), run_count);
                run_count = 0;
                last_line = None;
            }
            result.push(String::new());
            continue;
        }

        if let Some(prev) = last_line {
            if similarity(prev, line) >= threshold {
                run_count += 1;
                continue;
            } else {
                flush_run(&mut result, prev, run_count);
            }
        }

        last_line = Some(line);
        run_count = 1;
    }

    if let Some(prev) = last_line {
        flush_run(&mut result, prev, run_count);
    }

    result
}

fn flush_run(result: &mut Vec<String>, line: &str, count: usize) {
    if count <= 2 {
        for _ in 0..count {
            result.push(line.to_string());
        }
    } else {
        result.push(line.to_string());
        result.push(format!("[... {count} similar lines]"));
    }
}

/// Simple character-level similarity ratio (0.0 to 1.0).
fn similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let max_len = a.len().max(b.len());
    if max_len == 0 {
        return 1.0;
    }
    let common = a
        .chars()
        .zip(b.chars())
        .filter(|(ca, cb)| ca == cb)
        .count();
    common as f64 / max_len as f64
}

/// Try to detect and summarize test output.
fn try_compress_test_output(content: &str, level: CompressionLevel) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();

    // Detect test frameworks: cargo test, jest, pytest, go test
    let has_test_markers = lines.iter().any(|l| {
        l.contains("test result:") || l.contains("Tests:") || l.contains("passed")
            || l.contains("PASS") || l.contains("FAIL") || l.starts_with("ok ")
            || l.contains("test ") && (l.contains("... ok") || l.contains("... FAILED"))
    });

    if !has_test_markers || lines.len() < 5 {
        return None;
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut summary_lines: Vec<String> = Vec::new();
    let mut in_failure = false;

    for line in &lines {
        if line.contains("... ok") || line.contains("PASS") {
            passed += 1;
        } else if line.contains("FAILED") || line.contains("FAIL") {
            failed += 1;
            in_failure = true;
            failures.push(line.to_string());
        } else if line.contains("ignored") || line.contains("skipped") || line.contains("pending") {
            skipped += 1;
        } else if line.contains("test result:") || line.contains("Tests:") {
            summary_lines.push(line.to_string());
        } else if in_failure {
            // Capture failure context
            failures.push(line.to_string());
            if line.trim().is_empty() || failures.len() > 20 {
                in_failure = false;
            }
        }
    }

    let max_failures = match level {
        CompressionLevel::Light => 10,
        CompressionLevel::Medium => 5,
        CompressionLevel::Aggressive => 3,
    };

    let mut result = vec![format!(
        "[Test Summary] {passed} passed, {failed} failed, {skipped} skipped ({} total)",
        passed + failed + skipped
    )];

    if !failures.is_empty() {
        result.push(String::new());
        result.push("Failures:".to_string());
        for f in failures.iter().take(max_failures) {
            result.push(format!("  {f}"));
        }
        if failures.len() > max_failures {
            result.push(format!(
                "  [... {} more failure lines]",
                failures.len() - max_failures
            ));
        }
    }

    for sl in &summary_lines {
        result.push(sl.clone());
    }

    Some(result.join("\n"))
}

/// Try to detect and summarize compiler errors/warnings.
fn try_compress_compiler_output(content: &str, level: CompressionLevel) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();

    let error_count = lines.iter().filter(|l| l.contains("error[") || l.contains("error:")).count();
    let warning_count = lines.iter().filter(|l| l.contains("warning:") || l.contains("warn[")).count();

    if error_count + warning_count < 3 {
        return None;
    }

    let max_errors = match level {
        CompressionLevel::Light => 20,
        CompressionLevel::Medium => 10,
        CompressionLevel::Aggressive => 5,
    };

    let mut result = vec![format!(
        "[Compiler Summary] {error_count} errors, {warning_count} warnings"
    )];

    // Collect unique error messages
    let mut seen_errors = std::collections::HashSet::new();
    let mut error_lines: Vec<String> = Vec::new();

    for line in &lines {
        if (line.contains("error[") || line.contains("error:")) && error_lines.len() < max_errors {
            let normalized = line.trim().to_string();
            if seen_errors.insert(normalized.clone()) {
                error_lines.push(normalized);
            }
        }
    }

    if !error_lines.is_empty() {
        result.push(String::new());
        result.push("Errors:".to_string());
        for e in &error_lines {
            result.push(format!("  {e}"));
        }
        if error_count > error_lines.len() {
            result.push(format!(
                "  [... {} more errors]",
                error_count - error_lines.len()
            ));
        }
    }

    Some(result.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi() {
        let input = "\x1b[32m INFO\x1b[0m hello world";
        let result = strip_ansi(input);
        assert_eq!(result, " INFO hello world");
    }

    #[test]
    fn deduplicates_exact_lines() {
        let lines = vec!["ok", "ok", "ok", "ok", "ok", "done"];
        let result = dedup_lines(&lines, 1.0);
        assert!(result.len() < 6);
        assert!(result.iter().any(|l| l.contains("similar")));
    }

    #[test]
    fn truncates_long_output() {
        let long_content: String = (0..500).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let result = compress_output(&long_content, CompressionLevel::Medium);
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() <= 82); // 80 + dedup annotations
    }

    #[test]
    fn summarizes_test_output() {
        let test_output = "test auth::login ... ok\ntest auth::logout ... ok\ntest auth::register ... FAILED\ntest result: 2 passed; 1 failed\n";
        let result = compress_output(test_output, CompressionLevel::Medium);
        assert!(result.contains("[Test Summary]"));
        assert!(result.contains("2 passed"));
    }
}
