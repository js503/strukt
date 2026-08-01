# M2 Local Terminal Validation

- Date opened: 2026-07-31
- Branch: `feat/m2-terminal`
- Pull request: [#8](https://github.com/js503/strukt/pull/8)
- Tracking issue: [#7](https://github.com/js503/strukt/issues/7)
- Local platform: macOS on Apple silicon
- Rust: 1.97.1
- Scope: M2.3 local ephemeral PTY/ConPTY terminals

This evidence remains in progress until the implementation head passes hosted
macOS, Ubuntu, and Windows jobs and the native bundled-app walkthrough is complete.

## Deterministic Terminal Smoke Contract

The headless `--terminal-smoke <existing-root>` workflow uses the separately built
`terminal-fixture` executable rather than the user's shell. It verifies native PTY
spawn, Unicode input/output, ANSI cell state, resize propagation into a renderer
snapshot, independent process exit, a live quiet pane during load, 64 MiB of
bounded transport progress, per-pane and aggregate drain budgets, graceful
termination, a nested three-pane split, stopped-only restoration, and absence of
workspace-local `.strukt` metadata.

On success it exits zero and prints exactly:

```text
strukt terminal smoke: pty, unicode, ansi, resize, isolation, bounds, and restore passed
```

Launch-mode tests reject missing roots, nonexistent roots, extra arguments, and
near-match flags. The smoke has an internal 30-second deadline and the CI step has
a two-minute outer timeout.

## Local Verification

| Command | Result |
|---|---|
| `forj check /Users/jessie/Development/strukt` | Pass; governed primary checkout is valid |
| `git diff --check` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --all-targets --locked --offline` | Pass; 274 test executions, 0 failures |
| `cargo build -p strukt-app --locked --offline` | Pass |
| `cargo build -p strukt-terminal --bin terminal-fixture --locked --offline` | Pass |
| `target/debug/strukt-app --terminal-smoke <temporary-fixture>` | Pass in about four seconds; exact marker emitted |
| `cargo check -p strukt-app --target x86_64-unknown-linux-gnu --locked` | Pass |
| `cargo clippy -p strukt-terminal -p strukt-persistence -p strukt-workspace -p strukt-app --target x86_64-pc-windows-msvc --all-targets --locked --offline -- -D warnings` | Pass |

Only the previously documented transitive `block 0.1.6` future-compatibility
warning remains.

## Hosted CI Contract

The macOS 14, Ubuntu 24.04, and Windows Server 2022 jobs build the deterministic
fixture, create an isolated existing directory, run the exact smoke command,
require the exact marker, and reject `.strukt` metadata. Windows therefore runs
the native ConPTY contract rather than relying on cross-compilation alone.

Hosted run and job links are pending the next push.

## Native macOS Walkthrough

The raw development executable starts its Iced/WGPU event loop successfully. The
current macOS automation bridge cannot address a raw, non-bundled executable as an
application, so no visual or accessibility certification is claimed yet. Task 11
will create/use a native app bundle and record the complete interactive walkthrough.

## Review Status

Full-slice review is pending. Focused verification currently covers process cleanup
on runtime drop, stale generation rejection, bounded queue accounting, fair drain
budgets, parser and OSC limits, Unicode and wide-cell invariants, paste consent,
link scheme allowlisting plus second-action opening, stopped-only persistence, and
cross-target compilation.
