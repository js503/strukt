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

The workspace contains eleven passing tests:

- four capability-registry tests
- two semantic-theme tests
- three framework-independent shell-state tests
- two native-application wiring tests

Each behavioral suite was observed failing for the expected missing behavior before
its implementation was added.

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

## Remaining Acceptance Evidence

- macOS, Windows, and Linux hosted CI results
- native Windows window launch and shortcut inspection
- follow-up accessibility and IME prototypes described above

The Windows MSVC and Linux GNU cross-target checks pass from the macOS development
machine. They provide compile evidence, but do not replace native execution or the
hosted CI matrix.

Hosted CI could not be triggered during this validation because the local
repository has no Git remote configured.

ADR 0001 remains proposed until its stated validation gates have evidence.
