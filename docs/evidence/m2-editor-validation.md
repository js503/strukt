# M2 Editor Validation

- Date opened: 2026-07-31
- Branch: `feat/m2-editor`
- Pull request: [#6](https://github.com/js503/strukt/pull/6)
- Tracking issue: [#5](https://github.com/js503/strukt/issues/5)
- Local platform: macOS on Apple silicon
- Rust: 1.97.1
- Scope: M2.2 local editor workstream

This evidence record is intentionally incomplete until the final implementation
SHA passes the local, hosted, manual, and agentic-review gates in Task 10.

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
| `cargo test -p strukt-app --quiet` | Pass; 77 tests, 0 failures |
| `cargo clippy -p strukt-app --all-targets -- -D warnings` | Pass |
| `cargo test --workspace --all-targets` | Pass before the smoke slice; 0 failures |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass before the smoke slice |
| `cargo run -p strukt-app --quiet -- --editor-smoke <temporary-fixture>` | Pass; exact marker emitted |

The complete locked/offline gate, Linux cross-target check, final clean-tree smoke,
and exact test count will be recorded in Task 10.

## Hosted CI Contract

The macOS 14, Ubuntu 24.04, and Windows Server 2022 matrix jobs create the same
UTF-8 fixture, require a zero exit code and the exact marker, and reject any
workspace-local `.strukt` metadata. The workflow uses the smoke's deterministic
in-memory recovery key; it never reads or writes a runner credential store. The
existing workspace-files smoke remains on all three systems and the native startup
smoke remains on Windows.

Hosted run URLs and results are pending the final pushed implementation SHA.

## Manual macOS Checklist

Pending Task 10 validation:

- preview replacement and pinning; tabs and active-tab restoration;
- Unicode typing, IME composition, selection, and clipboard behavior;
- undo/redo and find/replace;
- safe save, dirty close, external clean reload, and all dirty-conflict actions;
- encrypted recovery across restart and save/discard cleanup;
- binary metadata, invalid UTF-8, and explicit large-file override;
- syntax themes and language override restoration;
- keyboard traversal, focus behavior, and accessibility labels;
- confirmation that neither the fixture nor repository gains `.strukt` metadata.

## Known Iced and Platform Limitations

- The editor uses Iced's native text editor. IME, accessibility, focus traversal,
  and clipboard behavior require manual platform validation in addition to reducer
  and domain tests.
- Cursor, selection, and scroll restoration are expressed through Iced editor
  actions; exact pixel scroll position is not a stable cross-platform contract.
- A Mac cannot execute the MSVC linker/assembler path. Native Windows CI is the
  authoritative Windows application and credential-provider gate.
- Cargo reports the previously documented future-compatibility warning for the
  transitive `block 0.1.6` dependency; Rust 1.97.1 gates still pass.

## Remaining M2 Gates

- Complete the locked/offline local gate and Linux cross-target check.
- Complete the manual macOS walkthrough.
- Complete full-slice agentic review and resolve important findings.
- Require macOS, Ubuntu, and Windows hosted checks on the final implementation SHA.
- Update the roadmap and tracker only after those gates pass.

