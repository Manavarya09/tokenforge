#!/usr/bin/env bash
# TokenForge PostToolUse hook — compresses tool output before it enters the context window.
#
# Install in ~/.claude/settings.json:
# {
#   "hooks": {
#     "PostToolUse": [
#       { "command": "/path/to/tokenforge/hooks/compress-output.sh" }
#     ]
#   }
# }
#
# The hook reads PostToolUse JSON from stdin and writes compressed output to stdout.
# If tokenforge is not installed or fails, it falls back to passing input through unchanged.

set -euo pipefail

# Find tokenforge binary
TOKENFORGE="${TOKENFORGE_BIN:-tokenforge}"
if ! command -v "$TOKENFORGE" &>/dev/null; then
  # Try common install locations
  for candidate in \
    "$HOME/.cargo/bin/tokenforge" \
    "/usr/local/bin/tokenforge" \
    "$(dirname "$0")/../core/target/release/tokenforge"; do
    if [[ -x "$candidate" ]]; then
      TOKENFORGE="$candidate"
      break
    fi
  done
fi

# Session ID from environment or fallback
SESSION_ID="${CLAUDE_SESSION_ID:-$(date +%Y%m%d)}"

# Read stdin into variable
INPUT=$(cat)

# If tokenforge is available, compress; otherwise pass through
if command -v "$TOKENFORGE" &>/dev/null || [[ -x "$TOKENFORGE" ]]; then
  echo "$INPUT" | "$TOKENFORGE" hook --session "$SESSION_ID" 2>/dev/null || echo "$INPUT"
else
  echo "$INPUT"
fi
