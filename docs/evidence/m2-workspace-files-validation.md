# M2 Workspace and Files Validation

- Date: 2026-07-29
- Branch: `feat/m2-workspace-files`
- Validated implementation SHA: `771b912d9109b60a91c324c1a86d5e24525581b7`
- Local platform: macOS 26.4.1 on Apple silicon
- Rust: 1.97.1
- Scope: M2 local workspace and files workstream only

The implementation, automated gates, hosted cross-platform workflow, manual macOS
walkthrough, and full-slice agentic review are complete. This record validates the
implementation commit above; the evidence-only completion commit follows it.

## Automated Local Verification

| Command | Result |
|---|---|
| `forj check .` from the primary checkout | Pass; governed manifest found |
| `git diff --check` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --all-targets --locked --offline` | Pass; 149 test executions, 0 failures |
| `cargo build -p strukt-app --locked --offline` | Pass |
| `cargo run -p strukt-app --locked --offline -- --workspace-files-smoke "$PWD"` with a temporary `strukt-smoke.txt` sentinel | Pass; exact marker emitted and process exited zero |
| `cargo check -p strukt-app --target x86_64-unknown-linux-gnu --locked --offline` | Pass |
| `cargo check -p strukt-app --target x86_64-pc-windows-msvc --locked --offline` | Blocked locally; the Mac does not provide `ml64.exe` |
| Parse `.github/workflows/ci.yml` with Ruby Psych | Pass |

The deterministic smoke emitted:

```text
strukt workspace files smoke: open, discovery, and persistence passed
```

The smoke runs its filesystem workflow on a headless Tokio runtime. It opens the
supplied folder, discovers the exact workflow-created `strukt-smoke.txt` file, and
saves and reloads the workspace state through an isolated temporary application-data
store. On success the CLI prints the marker and `main` returns zero; the Iced reducer
separately requests `iced::exit()` when it receives a successful smoke-completion
message. The workflow leaves no workspace-local metadata. Unit coverage also
rejects missing paths, extra arguments, near-match flags, and near-match sentinel
names.

Cargo continues to report the previously documented future-compatibility warning
for transitive dependency `block 0.1.6`. It does not fail the Rust 1.97.1 gates.

## Hosted CI Results

GitHub Actions run
[`30521125211`](https://github.com/js503/strukt/actions/runs/30521125211)
validated the implementation SHA on every supported hosted operating system.

| Hosted job | Result | Run URL |
|---|---|---|
| macOS 14 | Pass | [job `90801530084`](https://github.com/js503/strukt/actions/runs/30521125211/job/90801530084) |
| Ubuntu 24.04 | Pass | [job `90801529815`](https://github.com/js503/strukt/actions/runs/30521125211/job/90801529815) |
| Windows Server 2022 | Pass | [job `90801529787`](https://github.com/js503/strukt/actions/runs/30521125211/job/90801529787) |

Each job passed formatting, strict Clippy, tests, native application build, and the
platform-appropriate workspace-files smoke. Windows additionally passed a native
application launch smoke.

## Manual macOS Validation

| Check | Result |
|---|---|
| Open a temporary local folder through the native folder dialog | Pass; opened `/private/tmp/strukt-m2-ui-fixture` |
| Explorer shows real files and keeps the file browser readily accessible | Pass; real fixture entries remained available in the Files activity |
| Create, rename, duplicate, and confirm destructive workflows | Pass; created `created.txt`, renamed it to `renamed.txt`, duplicated it to `copy.txt`, confirmed Trash targeting, and verified the permanent-delete escalation before canceling |
| Create a file outside `strukt`; watcher refreshes the explorer | Pass; externally created `external.txt` appeared without a manual refresh |
| Default search excludes a newly ignored file | Pass; `secretunique` returned no result while the containing file was ignored |
| Enabling ignored files reveals the ignored file where applicable | Pass; the ignored result appeared after enabling search inclusion and ignored explorer entries were visibly muted |
| Quick open and content search return expected local-file results | Pass; Quick Open focused correctly and continued to exclude ignored files independently |
| Restart restores the workspace and explorer visibility preferences | Pass; the fixture and hidden/ignored toggles restored |
| Opened fixture and repository contain no `.strukt` path after the walkthrough | Pass |

## Review, Known Limitations, and Deferred Gates

- Full-slice security and correctness review completed with no remaining critical,
  important, or minor code findings after the review fixes.
- OS Trash currently fails closed with `TrashUnavailable`; it never falls back to
  permanent deletion. Permanent deletion remains a separate explicit,
  capability-confined confirmation path.
- On Windows, rename, move, and duplicate publication fail closed until a safe,
  atomic, capability-relative no-replace adapter is available. Create and explicit
  permanent delete remain capability-confined. Unix/macOS rename and duplicate use
  no-replace publication.
- The Windows MSVC check cannot be completed locally because the Mac lacks the
  Microsoft assembler; the native Windows hosted job is the required evidence.
- M2 editor buffers, syntax and language-server support, local PTY/ConPTY
  terminals, editor/terminal restoration, and cross-feature responsiveness remain
  separate workstreams.
- Cross-workstream integration and sustained terminal-output testing remain
  pending until the editor, language, and terminal foundations exist.
- Human Windows visual, accessibility, IME, packaging, installation, and complete
  keyboard-workflow validation remain M9 public-alpha gates.
- Cargo continues to warn that transitive `block 0.1.6` will be rejected by a
  future Rust version; this is tracked as dependency maintenance and did not fail
  Rust 1.97.1 verification.
