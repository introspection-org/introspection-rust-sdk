#!/bin/bash
# Setup git hooks for Rust SDK

set -e

HOOK_DIR=".git/hooks"
HOOK_FILE="$HOOK_DIR/pre-commit"
SOURCE_HOOK="hooks/pre-commit"

if [ ! -d "$HOOK_DIR" ]; then
    echo "Error: .git/hooks directory not found. Are you in a git repository?"
    exit 1
fi

if [ ! -f "$SOURCE_HOOK" ]; then
    echo "Error: Source hook file not found: $SOURCE_HOOK"
    exit 1
fi

cp "$SOURCE_HOOK" "$HOOK_FILE"
chmod +x "$HOOK_FILE"

echo "✓ Pre-commit hook installed successfully!"
echo "The hook will run automatically on git commit."
