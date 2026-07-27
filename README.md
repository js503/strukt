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

- Stage: native shell foundation implementation
- Current foundation: GPU-rendered Focus + Context workspace shell
- Milestone: M1 in progress

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

The current executable is a shell foundation. File entries, terminal output, and AI
context are representative views; real filesystem, PTY, SSH, and provider behavior
belong to later milestones.

## Verification

- Run `forj check .` to verify the governed repository manifest.
- Follow the verification requirements in the active spec and implementation plan.
- See `docs/evidence/m1-native-shell-validation.md` for native-window validation.

## Pull Request Expectations

- Link the issue
- Document verification
- Summarize agentic review findings
