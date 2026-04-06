use crate::CompressionLevel;

/// Compress conversation history using extractive compression.
///
/// Scores each turn by recency, code references, decisions, and errors.
/// High-scoring turns kept in full, low-scoring turns summarized to one line.
/// First turn and last 3 turns are always preserved.
pub fn compress_conversation(content: &str, level: CompressionLevel) -> String {
    let turns = parse_turns(content);

    if turns.len() <= 4 {
        return content.to_string();
    }

    let keep_recent = match level {
        CompressionLevel::Light => 5,
        CompressionLevel::Medium => 3,
        CompressionLevel::Aggressive => 2,
    };

    let score_threshold = match level {
        CompressionLevel::Light => 0.3,
        CompressionLevel::Medium => 0.5,
        CompressionLevel::Aggressive => 0.7,
    };

    let total = turns.len();
    let mut result = Vec::new();

    for (i, turn) in turns.iter().enumerate() {
        let is_first = i == 0;
        let is_recent = i >= total.saturating_sub(keep_recent);
        let score = score_turn(turn, i, total);

        if is_first || is_recent {
            // Always keep in full
            result.push(turn.raw.clone());
        } else if score >= score_threshold {
            // High relevance — keep but maybe trim
            result.push(trim_turn(turn, level));
        } else {
            // Low relevance — one-line summary
            let summary = summarize_turn(turn);
            result.push(summary);
        }
    }

    result.join("\n\n")
}

struct Turn {
    role: String,
    raw: String,
    has_code_block: bool,
    has_file_path: bool,
    has_decision: bool,
    has_error: bool,
    line_count: usize,
}

fn parse_turns(content: &str) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut current_role = String::new();
    let mut current_lines: Vec<&str> = Vec::new();

    let role_prefixes = ["User:", "Assistant:", "Human:", "AI:", "user:", "assistant:"];

    for line in content.lines() {
        let is_role_line = role_prefixes.iter().any(|p| line.starts_with(p));

        if is_role_line && !current_lines.is_empty() {
            turns.push(build_turn(&current_role, &current_lines));
            current_lines.clear();
        }

        if is_role_line {
            current_role = line.split(':').next().unwrap_or("").to_string();
        }

        current_lines.push(line);
    }

    if !current_lines.is_empty() {
        turns.push(build_turn(&current_role, &current_lines));
    }

    turns
}

fn build_turn(role: &str, lines: &[&str]) -> Turn {
    let raw = lines.join("\n");
    let has_code_block = raw.contains("```");
    let has_file_path = raw.contains(".rs")
        || raw.contains(".ts")
        || raw.contains(".py")
        || raw.contains(".js")
        || raw.contains("/src/")
        || raw.contains("\\src\\");
    let has_decision = raw.contains("I'll ")
        || raw.contains("Let's ")
        || raw.contains("We should")
        || raw.contains("The fix is")
        || raw.contains("I've decided")
        || raw.contains("Going with");
    let has_error = raw.contains("error")
        || raw.contains("Error")
        || raw.contains("failed")
        || raw.contains("FAILED")
        || raw.contains("panic");

    Turn {
        role: role.to_string(),
        raw,
        has_code_block,
        has_file_path,
        has_decision,
        has_error,
        line_count: lines.len(),
    }
}

fn score_turn(turn: &Turn, index: usize, total: usize) -> f64 {
    let mut score = 0.0;

    // Recency (exponential decay)
    let recency = index as f64 / total as f64;
    score += recency * 0.3;

    // Content signals
    if turn.has_code_block {
        score += 0.3;
    }
    if turn.has_file_path {
        score += 0.15;
    }
    if turn.has_decision {
        score += 0.2;
    }
    if turn.has_error {
        score += 0.25;
    }

    score.min(1.0)
}

fn trim_turn(turn: &Turn, level: CompressionLevel) -> String {
    let max_lines = match level {
        CompressionLevel::Light => 30,
        CompressionLevel::Medium => 15,
        CompressionLevel::Aggressive => 8,
    };

    if turn.line_count <= max_lines {
        return turn.raw.clone();
    }

    let lines: Vec<&str> = turn.raw.lines().collect();
    let mut result: Vec<&str> = Vec::new();
    let mut in_code_block = false;

    for line in &lines {
        if line.contains("```") {
            in_code_block = !in_code_block;
            result.push(line);
            continue;
        }

        // Always keep code block contents
        if in_code_block {
            result.push(line);
            continue;
        }

        // Keep lines with decisions, errors, file paths
        if line.contains("error") || line.contains("fix") || line.contains("/")
            || line.starts_with("- ") || line.starts_with("* ")
            || line.starts_with("#")
        {
            result.push(line);
            continue;
        }

        // Keep up to max_lines
        if result.len() < max_lines {
            result.push(line);
        }
    }

    if turn.line_count > result.len() {
        result.push(&"[... trimmed]");
    }

    result.join("\n")
}

fn summarize_turn(turn: &Turn) -> String {
    let first_line = turn.raw.lines().next().unwrap_or("");
    let truncated = if first_line.len() > 100 {
        // Find a char boundary at or before 100 to avoid panicking on multi-byte UTF-8.
        // Slicing directly at byte 100 will panic if it falls inside a multi-byte character.
        let mut end = 100;
        while end > 0 && !first_line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &first_line[..end])
    } else {
        first_line.to_string()
    };

    format!("[{} — {} lines] {truncated}", turn.role, turn.line_count)
}
