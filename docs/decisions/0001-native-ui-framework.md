# ADR 0001: Native UI Framework for the Foundation Milestone

- Status: Accepted for the M1 foundation
- Date: 2026-07-26
- Decision owners: strukt maintainers

## Context

`strukt` requires a native, GPU-rendered Rust interface that can ship on macOS,
Windows, and Linux without Electron. The first executable milestone must validate
the workspace shell, semantic theming, keyboard interaction, and modular feature
boundaries while leaving room for custom editor and terminal widgets.

The framework is a foundational dependency, but the product must not allow one UI
library to leak into the workspace, session, remote, or plugin domain models.

## Options considered

### Iced 0.14

- MIT licensed
- explicitly supports Windows, macOS, and Linux
- uses `wgpu` for Vulkan, Metal, and DX12
- supports custom widgets and renderer-level extension
- provides async tasks and subscriptions
- remains experimental, so breaking changes and missing desktop behaviors are risks

Sources:

- <https://github.com/iced-rs/iced>
- <https://docs.rs/iced/0.14.0/iced/>

### GPUI

- purpose-built for a high-performance Rust editor
- hybrid retained/immediate GPU rendering
- proven inside Zed
- still pre-1.0
- its current standalone README directs users to macOS or Linux, which conflicts
  with the Windows requirement for the first public alpha

Source:

- <https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md>

### Floem

- MIT licensed
- supports Windows, macOS, and Linux
- GPU rendering and fine-grained reactivity
- project documentation states that it is still maturing and expects breaking
  changes before 1.0

Source:

- <https://github.com/lapce/floem>

### Slint

- broad desktop platform coverage and GPU acceleration
- production-oriented declarative UI tooling
- licensing choices introduce GPL, attribution, or commercial-license constraints
  that are unnecessary for a permissively licensed foundation

Sources:

- <https://slint.dev/get-started>
- <https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/>

## Decision

Use Iced 0.14 for the native-shell foundation milestone, pinned through the Cargo
lockfile and isolated inside `crates/strukt-app`.

Domain crates must expose framework-independent Rust types. Only `strukt-app` may
depend on Iced. Semantic theme tokens live in `strukt-theme` and are converted to
Iced styles at the application boundary.

This is a milestone validation decision, not an unconditional permanent commitment.

## Validation Results

Local macOS validation on 2026-07-26 confirms:

- the native `wgpu` window launches without Electron or a browser;
- Focus + Context regions, light/dark themes, resizing, and shell shortcuts work;
- Iced is isolated to `strukt-app`;
- the domain crates build and test without Iced dependencies;
- the application passes cross-target checks for Windows MSVC and Linux GNU;
- the idle process sample reports `0.0%` CPU and `108752` KB RSS after extended
  idle.

The inspection also identified two open framework risks:

- rendered Iced controls were not exposed as individually addressable elements in
  the macOS accessibility inspection;
- IME behavior cannot be validated until M2 introduces editable text.

The complete evidence, screenshot, mitigation path, and measurement limitations are
recorded in
[`docs/evidence/m1-native-shell-validation.md`](../evidence/m1-native-shell-validation.md).

GitHub Actions
[run 30415817787](https://github.com/js503/strukt/actions/runs/30415817787)
passed compilation and tests on macOS, Windows, and Linux. The Windows Server 2022
job additionally launched the real native executable, reached the Iced event loop,
printed the required success marker, and exited cleanly.

## Validation gates

This M1 acceptance is supported by the following completed gates:

1. the shell builds on macOS, Windows, and Linux CI;
2. macOS manual validation and the Windows-native hosted smoke gate exercise the
   Iced window, renderer, and event loop;
3. platform-command shortcut tests pass natively on macOS and Windows;
4. light and dark semantic tokens style every shell surface;
5. domain state remains outside the Iced application crate;
6. startup and idle resource measurements are recorded;
7. IME and accessibility risks are documented with a concrete mitigation path.

Human Windows visual QA remains mandatory for M9 public-alpha readiness. Before M2
accepts an editor or terminal widget, the framework decision must be revisited
against custom-widget, accessibility, focus-order, keyboard-navigation, and IME
prototypes. M1 acceptance is not an unconditional permanent framework commitment.

If Iced fails a later gate, keep the domain crates and evaluate Floem or a focused
`winit`/`wgpu` shell without changing workspace-domain APIs.

## Consequences

- The first milestone can produce a cross-platform executable quickly.
- UI framework churn is contained within one crate.
- The terminal renderer will require a custom widget in a later milestone.
- The team accepts Iced's experimental status for the M1 foundation while retaining
  explicit M2 and M9 revalidation gates.
