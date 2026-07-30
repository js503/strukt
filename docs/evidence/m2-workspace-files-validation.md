# M2 Workspace and Files Validation

- Date: 2026-07-29
- Branch: `feat/m2-workspace-files`
- Final commit SHA: Pending final review and evidence commit
- Local platform: macOS 26.4.1 on Apple silicon
- Rust: 1.97.1
- Scope: M2 local workspace and files workstream only

This is a draft evidence record. Local automated results below were observed on the
working tree. Hosted CI, the final commit SHA, agentic review, and the manual macOS
walkthrough remain pending and must be filled in before this workstream is marked
complete.

## Automated Local Verification

| Command | Result |
|---|---|
| `forj check .` from the primary checkout | Pass; governed manifest found |
| `git diff --check` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --locked --offline` | Pass; 125 tests, 0 failures |
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

Hosted CI has not run against the final Task 10 commit.

| Hosted job | Result | Run URL |
|---|---|---|
| macOS 14 | Pending | Pending |
| Ubuntu 24.04 | Pending | Pending |
| Windows Server 2022 | Pending | Pending |

The native workflow now creates the same sentinel fixture and requires the exact
success marker on all three operating systems. Do not replace these pending entries
until the final-head jobs have completed.

## Manual macOS Validation

The Mac is available, but the final interactive walkthrough is intentionally
pending owner/orchestrator execution.

| Check | Result |
|---|---|
| Open a temporary local folder through the native folder dialog | Pending |
| Explorer shows real files and keeps the file browser readily accessible | Pending |
| Create and rename files outside `strukt`; watcher refreshes the explorer | Pending |
| Default search excludes a newly ignored file | Pending |
| Enabling ignored files reveals the ignored file where applicable | Pending |
| Quick open and content search return expected local-file results | Pending |
| Restart restores the workspace and explorer visibility preferences | Pending |
| Opened repository contains no `.strukt` path after the walkthrough | Pending |

## Pending Review and Deferred Gates

- Complete agentic review of the full M2 workspace/files diff is pending.
- Hosted macOS, Linux, and Windows jobs must pass at the final commit.
- The Windows MSVC check cannot be completed locally because the Mac lacks the
  Microsoft assembler; the native Windows hosted job is the required evidence.
- M2 editor buffers, syntax and language-server support, local PTY/ConPTY
  terminals, editor/terminal restoration, and cross-feature responsiveness remain
  separate workstreams.
- Cross-workstream integration and sustained terminal-output testing remain
  pending until the editor, language, and terminal foundations exist.
- Human Windows visual, accessibility, IME, packaging, installation, and complete
  keyboard-workflow validation remain M9 public-alpha gates.
