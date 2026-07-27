# ADR 0001: Native UI Framework for the Foundation Milestone

- Status: Proposed for milestone validation
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

## Validation gates

Move this ADR to `Accepted` only after:

1. the shell builds on macOS, Windows, and Linux CI;
2. the macOS and Windows applications open a GPU-rendered window;
3. keyboard focus and shortcuts work across the activity rail and panels;
4. light and dark semantic tokens style every shell surface;
5. an advanced custom widget can be introduced without moving domain state into the
   application crate;
6. startup and idle resource measurements are recorded;
7. IME and accessibility risks are documented with a concrete mitigation path.

If Iced fails a gate, keep the domain crates and evaluate Floem or a focused
`winit`/`wgpu` shell without changing workspace-domain APIs.

## Consequences

- The first milestone can produce a cross-platform executable quickly.
- UI framework churn is contained within one crate.
- The terminal renderer will require a custom widget in a later milestone.
- The team accepts Iced's experimental status during validation.
