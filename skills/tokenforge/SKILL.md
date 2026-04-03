---
name: tokenforge
description: TokenForge — Full-stack LLM token optimization engine
---

# /tokenforge

TokenForge compresses ALL token sources in your Claude Code sessions — code, command output, conversation, JSON, and MCP schemas — with AST-aware intelligence.

## Commands

### `/tokenforge stats`
Show compression statistics for the current session.

```bash
tokenforge stats --json
```

### `/tokenforge analyze`
Analyze token usage patterns — identifies session type (debugging, code review, feature build) and gives optimization recommendations.

```bash
tokenforge analyze
```

### `/tokenforge budget [total]`
View or set per-category token budgets.

```bash
# View current budget
tokenforge budget --show

# Set total budget to 80,000 tokens
tokenforge budget --set 80000
```

### `/tokenforge quality`
Show compression quality score (0-100) for the current session.

```bash
tokenforge quality
```

### `/tokenforge learn`
Build a project relevance profile from git history and file access patterns.

```bash
tokenforge learn --project .
```

### `/tokenforge expand <hash>`
Retrieve the full original content by its blake3 hash. Use when compressed output has lost information you need.

```bash
tokenforge expand abc123def456
```

## How It Works

TokenForge sits in the PostToolUse hook and compresses tool output before it enters the context window:

1. **Code** — AST-aware folding: keeps signatures, types, imports; folds large function bodies
2. **Command output** — Strips ANSI, deduplicates similar lines, summarizes test/compiler output
3. **Conversation** — Scores turns by recency + relevance, compresses low-value turns
4. **JSON** — Schema extraction + array sampling, depth limiting
5. **MCP schemas** — Tiered virtualization: active tools get full schemas, others get names only

All originals are stored losslessly in SQLite (zstd-compressed). Use `tokenforge expand` to retrieve any original.

## Installation

```bash
# Build from source
cd tokenforge/core && cargo build --release

# Install
cp target/release/tokenforge ~/.cargo/bin/

# Add hook to Claude Code settings
# ~/.claude/settings.json:
# {
#   "hooks": {
#     "PostToolUse": [
#       { "command": "/path/to/tokenforge/hooks/compress-output.sh" }
#     ]
#   }
# }
```
