#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
CORE_DIR="$PROJECT_DIR/core"

echo "=== TokenForge Build ==="
echo "Building release binary..."

cd "$CORE_DIR"
cargo build --release

BINARY="$CORE_DIR/target/release/tokenforge"

if [[ -f "$BINARY" ]]; then
  SIZE=$(du -sh "$BINARY" | cut -f1)
  echo ""
  echo "Build successful!"
  echo "  Binary: $BINARY"
  echo "  Size:   $SIZE"
  echo ""
  echo "Run ./scripts/install.sh to install."
else
  echo "ERROR: Build failed — binary not found at $BINARY"
  exit 1
fi
