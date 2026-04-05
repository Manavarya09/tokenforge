# TokenForge

**Full-stack LLM token optimization engine — the best in the world.** The first tool that compresses ALL token sources — code, command output, conversation, JSON, and MCP schemas — with _real_ AST parsing (tree-sitter grammars), semantic diff compression, a built-in MCP server, one-command auto-setup, and lossless reversibility.

[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![npm](https://img.shields.io/npm/v/@masyv/tokenforge)](https://www.npmjs.com/package/@masyv/tokenforge)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Why TokenForge?

Every existing tool solves one slice of the token problem:
- **RTK** — command output only
- **lean-ctx** — lossy compression, no reversibility
- **claw-compactor** — file content only
- **context-pilot** — conversation pruning only

**TokenForge is the first full-stack optimizer**: a single Rust binary that handles every token source with content-aware compression, lossless storage, quality measurement, and — uniquely — works as a proper **MCP server** that Claude can call directly.

## Benchmarks (v0.2.0, aggressive)

```
Engine                  Type         Tokens  → After   Saved%   Quality   ns/token
──────────────────────────────────────────────────────────────────────────────────
code:rust (AST)         code:rust      762      549    28.0%     88.0       3124
code:python (AST)       code:python    643      521    19.0%     88.0       3333
command_output          output         422       18    95.7%     75.0       3016
json_payload            json           302       55    81.8%     88.0       3404
mcp_schema              mcp_schema     390      149    61.8%     95.0       2686
```

Real tree-sitter AST parsing — not regex heuristics.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Claude Code Session                   │
│                                                         │
│  Tool Call → PostToolUse Hook → TokenForge → Compressed │
│                                    │                    │
│                              ┌─────┴─────┐              │
│                              │  Engine    │              │
│                              ├───────────┤              │
│                              │ Detect    │← Content type │
│                              │ Compress  │← AST-aware   │
│                              │ Store     │← SQLite+zstd │
│                              │ Score     │← Quality 0-100│
│                              └─────┬─────┘              │
│                                    │                    │
│                    ┌───────────────┼───────────────┐    │
│                    ▼               ▼               ▼    │
│              Code Compressor  Output Compressor  JSON   │
│              (tree-sitter)    (ANSI strip,      (schema │
│              12+ languages    dedup, summarize)  extract)│
│                    │               │               │    │
│                    ▼               ▼               ▼    │
│              Conversation    MCP Schema      Learning   │
│              (turn scoring,  (tiered:        (cross-    │
│              recency decay)  active/deferred) session)  │
└─────────────────────────────────────────────────────────┘
```

## Features

### What's New in v0.2.0
- **Real tree-sitter AST grammars** — Rust, Python, JavaScript, TypeScript. Surgical function body folding at exact CST node boundaries.
- **MCP server mode** — `tokenforge serve` runs as a proper JSON-RPC MCP server. Claude calls it directly via `claude_desktop_config.json`.
- **One-command setup** — `tokenforge setup` auto-configures `~/.claude/settings.json` atomically.
- **Semantic diff compression** — Detects when new content is >60% similar to stored content; stores only the diff.
- **Built-in benchmarks** — `tokenforge bench` with real metrics: tokens, savings %, quality score, ns/token.

### 5 Compression Engines

| Engine | What it does | Typical savings |
|--------|-------------|-----------------|
| **Code** | AST-aware folding — keeps signatures, types, imports; folds large function bodies | 40-70% |
| **Command Output** | Strips ANSI, deduplicates similar lines, summarizes test/compiler output | 50-80% |
| **Conversation** | Scores turns by recency + relevance, compresses low-value turns | 30-60% |
| **JSON** | Schema extraction + array sampling, depth limiting | 60-90% |
| **MCP Schema** | Tiered virtualization — active tools get full schemas, others get names only | 70-95% |

### Intelligence

- **Cross-session learning** — Builds project relevance profiles from git history and file access patterns
- **Pattern analysis** — Identifies session types (debugging, code review, feature build) and adjusts compression
- **Quality scoring** — Measures compression quality 0-100, auto-adjusts aggressiveness
- **Per-category budgets** — Allocates token budget across conversation, tool output, code context, and MCP schemas

### Lossless Reversibility

Every original is stored in SQLite with zstd compression. Use `tokenforge expand <hash>` to retrieve any original content — nothing is ever lost.

## Installation

### From source (recommended)

```bash
git clone https://github.com/Manavarya09/tokenforge.git
cd tokenforge
./scripts/build.sh
./scripts/install.sh
```

### Manual

```bash
cd core
cargo build --release
cp target/release/tokenforge ~/.cargo/bin/
```

## Quick Start

### 1. Compress tool output

```bash
# Compress a file
tokenforge compress src/main.rs --type code --level medium

# Compress command output from stdin
cargo test 2>&1 | tokenforge compress --type output --level aggressive

# Auto-detect content type
cat package.json | tokenforge compress
```

### 2. Set up as Claude Code hook

Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "command": "~/.tokenforge/compress-output.sh"
      }
    ]
  }
}
```

Now every tool output is automatically compressed before entering the context window.

### 3. Monitor compression

```bash
# View session stats
tokenforge stats

# Analyze patterns
tokenforge analyze

# Check quality
tokenforge quality

# View as JSON
tokenforge stats --json
```

### 4. Retrieve originals

```bash
# Expand compressed content by hash
tokenforge expand abc123def456789
```

### 5. Configure budgets

```bash
# Set total token budget
tokenforge budget --set 80000

# View current allocation
tokenforge budget --show
```

### 6. Build project profile

```bash
# Learn from current project
tokenforge learn --project .
```

## CLI Reference

```
tokenforge — Full-stack LLM token optimization engine

USAGE:
    tokenforge [OPTIONS] <COMMAND>

COMMANDS:
    compress    Compress text/code/output for LLM consumption
    hook        PostToolUse hook mode (reads JSON from stdin)
    stats       Show compression statistics
    analyze     Analyze token usage patterns
    budget      View or set per-category token budgets
    learn       Build project relevance profile
    quality     Show compression quality score
    expand      Expand compressed content by hash
    serve       Run as MCP server over stdio (JSON-RPC 2.0)
    setup       Auto-install PostToolUse hook into ~/.claude/settings.json
    bench       Benchmark all compression engines with real metrics

OPTIONS:
    --json           Output as JSON
    -v, --verbose    Verbose logging
    --db-path        SQLite database path [default: ~/.tokenforge/tokenforge.db]
    -h, --help       Print help
    -V, --version    Print version
```

## MCP Server Mode

Add TokenForge as an MCP server so Claude can compress on-demand:

**`claude_desktop_config.json`:**
```json
{
  "mcpServers": {
    "tokenforge": {
      "command": "tokenforge",
      "args": ["serve"]
    }
  }
}
```

Claude now has access to 4 new tools: `tokenforge_compress`, `tokenforge_expand`, `tokenforge_stats`, `tokenforge_bench`.

## One-Command Setup

```bash
tokenforge setup            # auto-configures ~/.claude/settings.json
tokenforge setup --dry-run  # preview without writing
```

## Compression Levels

| Level | Code | Output | JSON | Use when |
|-------|------|--------|------|----------|
| `light` | Keep 100+ line functions | Max 200 lines | Depth 5 | Early in session, exploring |
| `medium` | Fold 50+ line functions | Max 80 lines | Depth 3 | Normal development |
| `aggressive` | Fold 20+ line functions | Max 30 lines | Depth 2 | Context pressure, debugging |

## MCP Tools

TokenForge exposes 4 MCP tools for programmatic access:

- `tokenforge_stats` — Compression statistics
- `tokenforge_expand` — Retrieve original content by hash
- `tokenforge_budget` — View/set token budgets
- `tokenforge_expand_tool` — Retrieve full MCP tool schema

## Performance

- **< 5ms** for small outputs (< 1KB)
- **< 50ms** for large outputs (> 100KB)
- **< 15MB** binary size (release, stripped, LTO)
- **Zero runtime dependencies** — single static binary

## Tech Stack

- **Rust** — Zero-cost abstractions, no GC pauses in the hot path
- **tree-sitter** — Industrial-grade AST parsing for 12+ languages
- **SQLite** (rusqlite, bundled) — Full-fidelity state persistence
- **zstd** — Fast compression for stored originals
- **tiktoken** (cl100k_base) — Accurate OpenAI-compatible token counting
- **blake3** — Fast content hashing for deduplication

## License

MIT

---

Built by [@masyv](https://github.com/Manavarya09) as a Claude Code plugin.
