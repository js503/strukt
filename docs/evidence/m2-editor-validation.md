# M2 Editor Validation

- Date opened: 2026-07-31
- Branch: `feat/m2-editor`
- Pull request: [#6](https://github.com/js503/strukt/pull/6)
- Tracking issue: [#5](https://github.com/js503/strukt/issues/5)
- Local platform: macOS on Apple silicon
- Rust: 1.97.1
- Scope: M2.2 local editor workstream
- Validated implementation SHA: `fc9f7f0dbec6236aa315ce11ff65d111cc496e40`

The implementation SHA passed the local, hosted, manual, and agentic-review gates
defined in Task 10. The evidence-only completion commit must also pass the hosted
matrix before pull-request merge readiness.

## Deterministic Editor Smoke Contract

The headless `--editor-smoke <fixture-root>` workflow requires an editable UTF-8
file named `strukt-editor-smoke.txt`. It opens the file through the retained
workspace capability, creates a preview, edits and pins it, undoes and redoes the
edit, round-trips encrypted recovery using an isolated in-memory key provider,
saves with the expected disk revision, verifies the saved bytes, and persists and
reloads the versioned editor workspace contribution from an isolated temporary
application-data directory. It also verifies that no `.strukt` path was created.

On success it exits zero and prints exactly:

```text
strukt editor smoke: open, edit, save, and restore passed
```

Launch-mode tests reject missing paths, empty paths, extra arguments, near-match
flags, binary sentinels, and fixtures without the exact sentinel.

## Local Verification

| Command | Result |
|---|---|
| `forj check /Users/jessie/Development/strukt` | Pass; governed primary checkout is valid |
| `git diff --check` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --all-targets --locked --offline` | Pass; 222 test executions, 0 failures |
| `cargo build -p strukt-app --locked --offline` | Pass |
| `cargo run -p strukt-app --locked --offline -- --editor-smoke <temporary-fixture>` | Pass for LF, CRLF, and UTF-8 BOM fixtures; exact marker emitted |
| `cargo check -p strukt-app --target x86_64-unknown-linux-gnu --locked --offline` | Pass |
| `cargo clippy -p strukt-editor -p strukt-fs -p strukt-persistence -p strukt-workspace -p strukt-platform --target x86_64-pc-windows-msvc --all-targets --locked --offline -- -D warnings` | Pass |

## Hosted CI Contract

The macOS 14, Ubuntu 24.04, and Windows Server 2022 matrix jobs create the same
UTF-8 fixture, require a zero exit code and the exact marker, and reject any
workspace-local `.strukt` metadata. The workflow uses the smoke's deterministic
in-memory recovery key; it never reads or writes a runner credential store. The
existing workspace-files smoke remains on all three systems and the native startup
smoke remains on Windows.

- Implementation run:
  [30677504889](https://github.com/js503/strukt/actions/runs/30677504889)
- macOS 14:
  [pass](https://github.com/js503/strukt/actions/runs/30677504889/job/91307653476)
- Ubuntu 24.04:
  [pass](https://github.com/js503/strukt/actions/runs/30677504889/job/91307653477)
- Windows Server 2022:
  [pass](https://github.com/js503/strukt/actions/runs/30677504889/job/91307653466),
  including the Windows atomic-save regression, editor executable smoke, and native
  startup smoke.

The Windows executable fixture uses a unique directory and byte-exact BOM-free UTF-8
input so PowerShell encoding and reusable runner paths cannot weaken the contract.

## Manual macOS Walkthrough

A native macOS application bundle was exercised against an isolated fixture. The
walkthrough passed:

- explorer access, preview replacement, tab pinning, and active-buffer focus;
- ASCII editing, selection, clipboard, undo/redo, and find/replace;
- safe save and the dirty-close prompt;
- clean external reload, dirty external conflict, disk comparison, reload, keep,
  and force-save actions;
- encrypted recovery across application restart;
- Rust syntax detection plus light and dark syntax themes;
- binary metadata presentation and oversized read-only preview with the visible
  full-file override;
- confirmation that neither the fixture nor repository gained `.strukt` metadata.

The automation driver stripped non-ASCII input before delivery, so native macOS
IME composition could not be claimed from that walkthrough. Unicode cursor,
selection, insertion, multiline, and CRLF behavior instead passed domain and Iced
adapter regression tests. Human IME certification remains a public-alpha platform
gate. Iced exposed only the window and title through the automation accessibility
tree, so complete screen-reader labels and traversal also remain a public-alpha
human gate.

## Known Iced and Platform Limitations

- The editor uses Iced's native text editor. IME, accessibility, focus traversal,
  and clipboard behavior require manual platform validation in addition to reducer
  and domain tests.
- Cursor, selection, and scroll restoration are expressed through Iced editor
  actions; exact pixel scroll position is not a stable cross-platform contract.
- A Mac cannot execute the MSVC linker/assembler path. Native Windows CI is the
  authoritative Windows application and credential-provider gate.
- GitHub reports that `actions/checkout@v4` targets deprecated Node.js 20 and is
  currently forced to Node.js 24. This runner notice does not fail the matrix but
  should be removed by a future workflow dependency update.
- Cargo reports the previously documented future-compatibility warning for the
  transitive `block 0.1.6` dependency; Rust 1.97.1 gates still pass.

## Agentic Review

The full slice was reviewed locally against the spec because delegated review was
disabled for this session. Important findings were resolved before the hosted run:

- stale successful saves can no longer delete recovery for newer unsaved content;
- Iced UTF-8 byte columns now convert safely to domain character offsets, including
  Unicode selection and CRLF backspace behavior;
- oversized previews stream and hash the complete file without allocating the
  complete payload, while validating full-file UTF-8;
- restored active tabs no longer reclaim focus after a later explicit file open;
- cursor, selection, scroll, replacement text, and find options persist and restore;
- Windows save publication uses a retained-directory-handle-resolved absolute path
  accepted by `SetFileInformationByHandle`, with a native replacement regression;
- Windows workspace-root lock behavior is tested separately from Unix replacement
  detection.

No unresolved critical or important review findings remain. M2 itself stays in
progress because language intelligence and local PTY/ConPTY terminal workstreams
remain.
