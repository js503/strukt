# strukt

## Purpose

`strukt` is an open-source, AI-native development interface.

It treats terminals, editors, AI agents, remote systems, Git, logs, documentation,
and developer workflows as first-class surfaces inside one native workspace. The
terminal is one view rather than the center of the product.

The long-term goal is to become the local-first operating interface for modern
software engineering: one context, every tool, and any model.

## Principles

- Open source first
- Local-first architecture
- Cloud optional, never required
- Native macOS, Windows, and Linux applications
- GPU-rendered performance without Electron
- Keyboard-first, scriptable, and reproducible workflows
- Model-agnostic AI
- MCP-first integrations
- Sandboxed, capability-based extensions

## Status

- Stage: SSH remote-workspace implementation
- Current foundation: native shell plus real local workspace, file, editor, and
  language workflows, ephemeral terminals, and native local persistent sessions
- Milestones: M1 through M3 complete; M4 is active and M5 remains on the
  public-alpha critical path

## Key Docs

- Roadmap: `docs/roadmap.md`
- Specs: `docs/specs/`
- Plans: `docs/plans/`
- Mockups: `docs/mockups/`
- Tracker: `docs/tracker.md`
- Process standards: `docs/process/`

## Local Development

Install Rust through `rustup`; the repository pins the required toolchain.

```bash
cargo run -p strukt-app
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

The native application can open a real local folder and expose its files through the
explorer. Hidden and ignored visibility are independent, persisted workspace
preferences. The explorer supports create and explicit permanent-delete workflows
through a retained workspace capability. Rename and duplicate use no-replace
publication on Unix and macOS; they currently fail closed on Windows until a safe
atomic adapter is available. OS Trash also currently fails closed as unavailable
and never falls back to permanent deletion. Quick open, bounded content search,
native filesystem watching, recent workspaces, and workspace restoration all use
the same local workspace state.

Workspace state is stored in the platform application-data directory, not in the
opened repository; `strukt` does not create a `.strukt` directory in a workspace.
The native editor adds preview and pinned tabs, Unicode-safe transactional editing,
bounded undo/redo, find and replace, syntax themes, safe revision-checked saves,
external-change reconciliation, encrypted crash recovery, and persisted editor
layout. Binary and invalid UTF-8 files use metadata views, while oversized text
opens as an explicit read-only preview before a full-file override. The local
terminal adds explicit default-shell startup, bounded scrollback,
GPU-rendered Unicode and ANSI cells, tabs, recursive splits, selection, clipboard
consent, explicit link opening, restart/close lifecycle controls, fair output
draining, and stopped-only presentation restoration. Terminal output, commands,
environment data, selections, and clipboard contents are never persisted.
Language intelligence discovers user-installed servers through one bounded,
language-agnostic LSP client. It supports exact approval for workspace commands,
diagnostics grouped in Problems, completion, hover, definition, persisted
enablement, explicit restart, capability-aware saves, deadline enforcement, and
stopped-only restoration without writing workspace metadata. M3 now adds named
local sessions, windows, split panes, authenticated per-user IPC, live detach and
reattach, bounded historical output, explicit batch restart/termination, and
stopped-only machine-restart restoration. M4 adds standard-OpenSSH remote
workspaces with explicit host/root entry, helper capability negotiation, direct
terminal fallback, root-confined files, revision-checked editing, search, Git,
tasks, language transport, reconnect isolation, and secret-free records. Remote
terminals are ephemeral in M4; persistent remote sessions belong exclusively to
M5. M6 onward is the post-alpha roadmap.

Open the **Connect** activity to enter an alias from your normal OpenSSH config and
an absolute or `~/`-relative Linux workspace root. strukt invokes the platform
`ssh` executable with separate validated arguments and preserves OpenSSH host-key,
agent, keychain, and authentication behavior. It never stores passwords, private
keys, passphrases, agent tokens, raw environments, or protocol payloads. If the
versioned helper is unavailable, the connection is labeled **Terminal only** and
the direct SSH terminal remains available without claiming persistence.

The deterministic M4 smoke uses the repository-owned fake OpenSSH adapter and the
real helper protocol; it requires no keys or external host:

```bash
fixture="$(mktemp -d)"
cargo build -p strukt-remote --bins
cargo build -p strukt-app
cargo run -p strukt-app -- --remote-smoke "$fixture"
test ! -e "$fixture/.strukt"
```

The deterministic M3 smoke accepts any existing folder and uses only the bundled
session helper and fixture:

```bash
fixture="$(mktemp -d)"
cargo build -p strukt-session --bin strukt-sessiond --bin session-fixture
cargo run -p strukt-app -- --session-smoke "$fixture"
```

The deterministic workspace-files smoke mode expects a folder containing
`strukt-smoke.txt`:

```bash
fixture="$(mktemp -d)"
printf 'strukt\n' > "$fixture/strukt-smoke.txt"
cargo run -p strukt-app -- --workspace-files-smoke "$fixture"
```

It opens and discovers the fixture, persists and reloads a workspace snapshot in an
isolated temporary store, prints a stable success marker, and exits.

The deterministic local-terminal smoke accepts any existing folder, launches only
the repository's terminal fixture, exercises native PTY/ConPTY behavior and bounded
load, rejects workspace metadata, and exits with an exact marker:

```bash
fixture="$(mktemp -d)"
cargo build -p strukt-terminal --bin terminal-fixture
cargo run -p strukt-app -- --terminal-smoke "$fixture"
```

The final M2 language and integration smokes use only repository-owned fixtures:

```bash
fixture="$(mktemp -d)"
printf 'strukt\n' > "$fixture/strukt-smoke.txt"
printf 'strukt\n' > "$fixture/strukt-editor-smoke.txt"
cargo build -p strukt-language --bin language-fixture
cargo run -p strukt-app -- --language-smoke "$fixture"
cargo run -p strukt-app -- --m2-integration-smoke "$fixture"
```

## Verification

- Run `forj check .` to verify the governed repository manifest.
- Follow the verification requirements in the active spec and implementation plan.
- See `docs/evidence/m1-native-shell-validation.md` for native-window validation.
- See `docs/evidence/m2-workspace-files-validation.md` for workspace/files
  validation and current platform limitations.
- See `docs/evidence/m2-editor-validation.md` for editor validation and current
  platform limitations.
- See `docs/evidence/m2-local-terminal-validation.md` for PTY/ConPTY, renderer,
  stress, native walkthrough, and current framework limitations.
- See `docs/evidence/m2-language-intelligence-validation.md` for language,
  cross-platform integration, native walkthrough, and full M2 review evidence.
- See `docs/evidence/m3-local-persistent-sessions-validation.md` for session
  service, protocol, lifecycle, security, native walkthrough, and matrix evidence.

## Pull Request Expectations

- Link the issue
- Document verification
- Summarize agentic review findings
