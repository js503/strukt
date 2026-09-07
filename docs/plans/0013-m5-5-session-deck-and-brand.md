# M5.5 Session Deck and Brand Implementation Plan

> **Execution:** Implement task-by-task with `superpowers:executing-plans`, use
> `superpowers:test-driven-development` for behavior changes, and complete the
> repository verification gates before recording completion.

**Goal:** Make the active persistent terminal session the default workspace
canvas, keep Files and editor work immediately accessible, and replace the
workspace-bar wordmark with the approved animated Bolt Structure mark.

**Assumptions:** Existing PTY, editor, local-session, remote-session, promotion,
and persistence lifecycles remain authoritative. This slice changes composition
and presentation only. Existing M5.5 changes on the active feature branch are
part of the same milestone and must be preserved.

**Architecture:** `strukt-shell` owns the terminal-first composition policy and
generic surface promotion. `strukt-app` maps session, file, editor, and drawer
views onto that state without taking ownership of lifecycle data. `strukt-ui`
owns the reusable theme-aware brand vector and its widget-local animation
clock. The logo schedules redraws only during its short active pass and becomes
fully static for reduced motion or an unfocused window.

**Tech stack:** Rust 2024, iced 0.14 canvas, `strukt-shell`, `strukt-ui`,
`strukt-theme`, existing PTY/session/editor services, forj verification.

---

## File map

- `crates/strukt-shell/src/state.rs`: default Session Deck state and Files
  sidebar behavior while a terminal session remains primary.
- `crates/strukt-shell/tests/shell_state.rs`: terminal-first default contracts.
- `crates/strukt-shell/tests/composition_state.rs`: activity/sidebar and
  supporting-surface promotion regressions.
- `crates/strukt-ui/src/brand.rs`: canonical Bolt Structure geometry, motion
  sampling, focus handling, and canvas renderer.
- `crates/strukt-ui/src/lib.rs`: public brand component and contract exports.
- `crates/strukt-ui/tests/component_contracts.rs`: geometry, cadence,
  reduced-motion, and theme-injection contracts.
- `crates/strukt-app/src/view/shell.rs`: unified workspace bar, session-owned
  canvas composition, and supporting editor drawer routing.
- `crates/strukt-app/src/view/sessions.rs`: compact session navigator.
- `crates/strukt-app/src/view/editor.rs`: supporting editor drawer and promoted
  editor surface recognition.
- `crates/strukt-app/src/view/mod.rs`: terminal-first session canvas and editor
  promotion composition.
- `crates/strukt-app/src/app.rs`: open-file-to-supporting-editor transition and
  registered surface identifiers.
- `crates/strukt-app/tests/visual_foundation_contract.rs`: workspace-bar,
  hierarchy, and terminal-first visual constants.
- `docs/tracker.md`: M5.5 spec and plan linkage.
- `docs/evidence/m5-5-visual-fidelity-validation.md`: final verification and
  native visual-review evidence.

### Task 1: Lock the approved artifacts

- [x] Add the approved Session Deck and Bolt Structure spec.
- [x] Write this implementation plan with exact ownership and verification.
- [x] Link spec `0011` and plan `0013` from the M5.5 tracker row.

### Task 2: Specify terminal-first shell behavior with failing tests

**Files:**
- Modify: `crates/strukt-shell/tests/shell_state.rs`
- Modify: `crates/strukt-shell/tests/composition_state.rs`

- [x] Assert that a fresh shell opens Sessions with the session canvas primary,
  the Sessions sidebar visible, the supporting drawer hidden, and normal motion
  enabled.
- [x] Assert that selecting Files changes the contextual sidebar while retaining
  the active session canvas.
- [x] Assert that a generic editor-supporting surface can open in the drawer,
  promote to split/full, and demote without losing the prior terminal canvas.
- [x] Run the focused shell tests and confirm they fail for the old Files-first
  state before implementing the policy.

### Task 3: Implement the shell composition policy

**Files:**
- Modify: `crates/strukt-shell/src/state.rs`

- [x] Change the default activity/sidebar/canvas to Sessions and opt into normal
  motion by default while retaining the explicit reduced-motion state.
- [x] Preserve a Sessions primary surface when Files changes only the contextual
  sidebar. Continue replacing the canvas for activities that explicitly own a
  primary tool surface.
- [x] Keep promotion and demotion generic so editor and terminal surfaces share
  the existing lifecycle-safe model.
- [x] Run the focused shell tests to green.

### Task 4: Specify and implement the Bolt Structure component

**Files:**
- Create: `crates/strukt-ui/src/brand.rs`
- Modify: `crates/strukt-ui/src/lib.rs`
- Modify: `crates/strukt-ui/tests/component_contracts.rs`

- [x] Add failing contracts for the 29-pixel mark, 42-pixel identity slot,
  six-sided structure, internal planes, lightning path, central node, 8.4-second
  cadence, long idle interval, focus pause, and zero-redraw reduced-motion path.
- [x] Implement canonical normalized geometry and a pure
  `brand_motion_sample` function so cadence behavior is deterministic in tests.
- [x] Implement an iced canvas program that uses semantic primary, muted, and
  accent colors; requests redraws only during the active pass; and pauses while
  unfocused.
- [x] Export an accessible icon-only `brand_mark` component and run the focused
  `strukt-ui` contracts to green.

### Task 5: Compose the native Session Deck

**Files:**
- Modify: `crates/strukt-app/src/view/shell.rs`
- Modify: `crates/strukt-app/src/view/sessions.rs`
- Modify: `crates/strukt-app/src/view/editor.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/tests/visual_foundation_contract.rs`

- [x] Add failing app contracts for the icon-only workspace identity, continuous
  top plane, terminal-first surface, compact 11-to-14-pixel chrome scale, and
  supporting editor height.
- [x] Replace the visible workspace-bar wordmark with `brand_mark`, keeping the
  execution-boundary path and command control on the same uninterrupted plane.
- [x] Render the active local or remote session as the dominant canvas whenever
  the shell surface says Sessions, independent of which contextual sidebar is
  selected.
- [x] Recompose Sessions as a compact boundary/session navigator and terminal
  deck without changing attach, detach, window, pane, reconnect, or persistence
  messages.
- [x] When a file successfully opens from the terminal-first workspace, open it
  in a compact editor-supporting drawer. Support split/full promotion and
  demotion through the generic shell actions.
- [x] Run focused app and integration tests to green.

### Task 6: Verify, document, and launch

**Files:**
- Modify: `docs/evidence/m5-5-visual-fidelity-validation.md`
- Modify: `docs/tracker.md`

- [x] Run `cargo fmt --all --check`.
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Run `cargo test --workspace`.
- [x] Run `bash scripts/check-ui-semantics.sh`.
- [x] Run `bash scripts/m5-5-visual-foundation-smoke.sh`.
- [x] Run `forj check .`.
- [x] Launch `cargo run -p strukt-app` and perform native review of the default
  local Session Deck, Files continuity, supporting editor drawer, local Git
  branch status, logo rest frame, and subtle motion.
- [ ] Complete the remaining light-theme, remote-host, active-session, promoted
  editor, and native screen-reader acceptance matrix.
- [x] Record exact command results and visual findings in the M5.5 evidence;
  leave the milestone `In review` until the full visual acceptance gate is met.

## Self-review

### Header gutter correction

Follow-up: integrate the command control as a flat, full-height header target
with hover/pressed feedback. Reserve the Explorer's one-pixel border inside its
layout so rounded rows cannot overpaint it; remove the rail's duplicate border
and top inset. Validate focused regression guards and a fresh native capture.

The native capture exposed a shrink-height painted header nested inside a
40-pixel outer container. Move height, vertical alignment, and internal padding
onto the painted container itself. Verify the regression guard and inspect a
rebuilt native window. This corrects the approved continuous-header contract;
no composition or session lifecycle changes are included.

- The plan covers every acceptance criterion in spec `0011` without expanding
  terminal, SSH, multiplexer, or editor lifecycle scope.
- State ownership remains outside the view layer; the brand renderer owns only
  local animation state.
- Every behavior change begins with a failing contract and ends with focused and
  workspace-wide verification.
