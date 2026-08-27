#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

output="$(cargo run -p strukt-app --locked --offline -- --m5-5-visual-foundation-smoke)"
printf '%s\n' "$output"
test "$output" = "M5.5 visual foundation smoke passed"
