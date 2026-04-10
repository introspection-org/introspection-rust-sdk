#!/usr/bin/env bash
# Run all examples. Env vars are loaded from .env if present (local dev),
# or injected by CI as environment secrets.
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Load .env if present (local dev only)
if [ -f .env ]; then
  set -a
  source .env
  set +a
fi

echo "==> Running openai_tool_call"
cargo run --example openai_tool_call --features openai

echo "==> Running openai_streaming"
cargo run --example openai_streaming --features openai

echo "==> Running logfire_introspection"
cargo run --example logfire_introspection --features logfire,openai

echo "==> Running responses_api_features"
cargo run --example responses_api_features --features openai

echo "==> Running logfire_dual_export"
cargo run --example logfire_dual_export --features logfire,openai

echo "==> All examples completed successfully"
