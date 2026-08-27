#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
view_root="$repo_root/crates/strukt-app/src/view"

if rg -n 'iced::Color|Color::from_rgb|#[0-9A-Fa-f]{6}' "$view_root"; then
  printf '%s\n' "UI semantic check failed: app views must consume theme tokens through strukt-ui"
  exit 1
fi

printf '%s\n' "strukt UI semantic ownership check passed"
