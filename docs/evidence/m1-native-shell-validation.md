# M1 Native Shell Validation

- Date: 2026-07-26
- Branch: `feat/native-shell-foundation`
- Local platform: macOS on Apple silicon
- UI framework: Iced 0.14 with `wgpu`
- Screenshot:
  [`m1-native-shell-macos.jpeg`](m1-native-shell-macos.jpeg)

## Automated Local Verification

The following commands pass locally:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p strukt-app
cargo check -p strukt-app --target x86_64-pc-windows-msvc
cargo check -p strukt-app --target x86_64-unknown-linux-gnu
```

The workspace contains sixteen passing tests:

- four capability-registry tests
- two semantic-theme tests
- three framework-independent shell-state tests
- seven native-application launch, shortcut, lifecycle, and wiring tests

Each behavioral suite was observed failing for the expected missing behavior before
its implementation was added.

The real macOS executable also passes the deterministic smoke path:

```bash
cargo run -p strukt-app -- --smoke-test
```

It opens the native Iced application, runs the event loop for three seconds, prints
`strukt smoke test: native event loop started`, and exits with status zero. A normal
interactive launch remains open beyond the smoke interval.

## macOS Window Verification

The locally built binary was launched inside a temporary application bundle so
macOS accessibility tooling could address the otherwise unbundled Cargo
executable. The wrapper was used only for inspection and is not a packaging
artifact.

| Check | Result |
|---|---|
| Native window opens without a browser or Electron | Pass |
| Activity rail, explorer, primary canvas, context panel, and drawer render | Pass |
| Selecting Files restores a closed explorer | Pass |
| Context and drawer visibility remain independent | Pass |
| Command+B toggles the explorer | Pass |
| Command+J toggles the drawer | Pass |
| Command+\ toggles the context panel | Pass |
| Light/dark switching redraws shell surfaces | Pass |
| Resizing preserves a usable primary canvas | Pass |

## Resource Observations

- The first temporary-bundle launch command returned after approximately 2.7
  seconds. This is an inspection observation, not a controlled time-to-first-frame
  benchmark.
- After extended idle, the process sample reported `0.0%` CPU and `108752` KB RSS.
- Cargo reports a future-compatibility warning for transitive dependency
  `block 0.1.6`. It does not fail Rust 1.97.1 builds, but must be revisited during
  framework upgrades.

## Framework Risks and Mitigations

### Accessibility

The macOS accessibility inspection exposed the native window chrome but did not
expose the rendered Iced controls as addressable accessibility elements. Coordinate
input was required for button inspection.

Before M2 accepts an editor or terminal interaction model, `strukt-app` must
prototype semantic labels, focus order, keyboard traversal, and accessible actions
without moving domain state out of the framework-independent crates.

### Input Method Editors

M1 has no editable text surface, so IME composition is not validated. M2 must test
composition, candidate selection, and committed text on macOS and Windows before an
editor widget is accepted.

### Custom Widgets

Iced remains isolated to `strukt-app`; all capability, theme, and shell state lives
in framework-independent crates. M2 must validate a custom terminal or editor
widget against this boundary before ADR 0001 is considered permanent.

## Windows Hosted Smoke Strategy

M1 uses a deterministic Windows-native startup smoke mode because the project does
not currently have a human-operated Windows environment. The hosted gate exercises
the real Iced executable, native window and renderer initialization, event loop,
clean runtime exit, and Windows-native platform-command shortcut tests.

The Windows job requires both status zero and
`strukt smoke test: native event loop started`; it fails after two minutes if the
process hangs.

This is native startup evidence, not visual QA. Human Windows visual,
accessibility, IME, packaging, and installation validation remain mandatory before
public-alpha readiness can be marked complete.

## Hosted CI Results

GitHub Actions
[run 30415817787](https://github.com/js503/strukt/actions/runs/30415817787)
passed on the M1 feature branch:

| Hosted job | Result | Duration |
|---|---|---|
| macOS 14 | Pass | 42 seconds |
| Ubuntu 24.04 | Pass | 30 seconds |
| Windows Server 2022 | Pass | 1 minute 9 seconds |

The Windows job ran all seven `strukt-app` tests natively, including the
Control-mapped platform shortcut tests. It then launched
`target\debug\strukt-app.exe --smoke-test`, emitted
`strukt smoke test: native event loop started`, and exited successfully.

The Windows MSVC and Linux GNU cross-target checks also pass from the macOS
development machine.

## Deferred Validation

- M2: custom editor or terminal widget boundary
- M2: accessibility semantics, focus order, keyboard traversal, and IME behavior
- Public alpha: human Windows visual, packaging, installation, and keyboard-workflow QA

These are explicit later-milestone gates rather than incomplete M1 acceptance
evidence.
