#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
CORE_DIR="$PROJECT_DIR/core"
BINARY="$CORE_DIR/target/release/tokenforge"

INSTALL_DIR="${INSTALL_DIR:-$HOME/.cargo/bin}"

echo "=== TokenForge Install ==="

# Check if binary exists
if [[ ! -f "$BINARY" ]]; then
  echo "Release binary not found. Building first..."
  "$SCRIPT_DIR/build.sh"
fi

# Install binary
echo "Installing to $INSTALL_DIR/tokenforge..."
mkdir -p "$INSTALL_DIR"
cp "$BINARY" "$INSTALL_DIR/tokenforge"
chmod +x "$INSTALL_DIR/tokenforge"

# Create data directory
mkdir -p "$HOME/.tokenforge"

# Set up hook
HOOK_SRC="$PROJECT_DIR/hooks/compress-output.sh"
HOOK_DST="$HOME/.tokenforge/compress-output.sh"
echo "Installing hook to $HOOK_DST..."
cp "$HOOK_SRC" "$HOOK_DST"
chmod +x "$HOOK_DST"

echo ""
echo "Installation complete!"
echo ""
echo "  Binary:  $INSTALL_DIR/tokenforge"
echo "  Hook:    $HOOK_DST"
echo "  Data:    $HOME/.tokenforge/"
echo ""
echo "To enable in Claude Code, add to ~/.claude/settings.json:"
echo '  {'
echo '    "hooks": {'
echo '      "PostToolUse": ['
echo "        { \"command\": \"$HOOK_DST\" }"
echo '      ]'
echo '    }'
echo '  }'
echo ""
echo "Verify: tokenforge --version"
