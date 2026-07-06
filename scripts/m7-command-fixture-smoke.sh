#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

if [[ ! -x target/debug/ai-mcp ]]; then
	cargo build -q -p ai-mcp --locked
fi

target/debug/ai-mcp route-command --dry-run --text "open browser to example dot com" \
	| grep -q '"intent": "open_url"'
target/debug/ai-mcp route-command --dry-run --text "open browser" \
	| grep -q '"command": "wmaker-open-browser"'
target/debug/ai-mcp route-command --dry-run --text "open terminal" \
	| grep -q '"command": "wmaker-open-terminal"'
target/debug/ai-mcp route-command --dry-run --text "open chrome" \
	| grep -q '"command": "wmaker-open-chrome"'
target/debug/ai-mcp route-command --dry-run --text "make a cylinder in Blender and render it" \
	| grep -q '"skill_id": "blender.procedural"'

echo "M7 command fixture smoke passed"
