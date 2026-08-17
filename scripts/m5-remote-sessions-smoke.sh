#!/usr/bin/env bash
set -euo pipefail

fixture="$(mktemp -d)"
# macOS Unix-domain sockets have a 103-byte path ceiling. Keep the private
# service fixture intentionally short while still using a unique mktemp root.
session_data="$(mktemp -d /tmp/strukt-m5.XXXXXX)"
cleanup() {
  rm -rf "$fixture" "$session_data"
}
trap cleanup EXIT

cargo build -p strukt-session --bin strukt-sessiond -p strukt-remote --bin fake-ssh --locked --offline
STRUKT_REMOTE_SESSION_DATA="$session_data" \
  cargo run -p strukt-app --locked --offline -- --remote-sessions-smoke "$fixture"
test ! -e "$fixture/.strukt"
