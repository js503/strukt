# M5.5 Activity Rail Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Unicode activity-rail glyphs and the narrow selection bar with one native vector family, an edge-to-edge square selection field, and the approved 2.5D top-down selected state.

**Architecture:** `strukt-ui` owns canonical icon geometry and a canvas-backed renderer with widget-local transition state. The activity button composes that renderer with the existing semantic theme and message path, so selecting an activity remains a normal `ShellAction::SelectActivity` and does not add application-wide animation state. The renderer draws the same paths for rest, depth, face, and highlight passes; reduced motion jumps directly to the final frame.

**Tech Stack:** Rust 2024, `iced` 0.14 canvas, `strukt-ui`, semantic `strukt-theme` tokens, existing M5.5 contract and smoke tests.

---

## File map

- `docs/specs/0010-m5-5-visual-and-interaction-foundation.md`: approved rail geometry and motion contract.
- `crates/strukt-ui/src/icon.rs`: canonical vector geometry, transition state, and canvas renderer.
- `crates/strukt-ui/src/components/button.rs`: 48-pixel activity target and edge-to-edge selected appearance.
- `crates/strukt-ui/src/components/mod.rs`: public rail constants.
- `crates/strukt-ui/src/lib.rs`: icon renderer and geometry contract exports.
- `crates/strukt-app/src/view/activity.rs`: zero-horizontal-padding rail composition.
- `crates/strukt-ui/tests/component_contracts.rs`: vector-family and selected-state component tests.
- `crates/strukt-app/tests/visual_foundation_contract.rs`: shell geometry regression tests.
- `docs/tracker.md`: active M5.5 plan linkage.
- `docs/evidence/m5-5-visual-fidelity-validation.md`: verification record for the refinement.

### Task 1: Lock the approved contract

**Files:**
- Modify: `docs/specs/0010-m5-5-visual-and-interaction-foundation.md`
- Modify: `docs/tracker.md`

- [x] **Step 1: Replace the obsolete narrow-indicator requirement**

Document a 48-by-48-pixel activity target, full-width square selected background,
no accent bar, one canonical vector geometry per icon, and a 180-millisecond 2.5D
top-down transition with a zero-duration reduced-motion result.

- [x] **Step 2: Link this plan from the tracker**

Append `plans/0012-m5-5-activity-rail-icons.md` to the M5.5 plan column without
changing the milestone from `In review`.

### Task 2: Write failing icon and geometry contracts

**Files:**
- Modify: `crates/strukt-ui/tests/component_contracts.rs`
- Modify: `crates/strukt-app/tests/visual_foundation_contract.rs`

- [x] **Step 1: Add the `strukt-ui` contract test**

Add assertions that every `Icon` exposes non-empty canonical vector geometry,
the activity target is `48.0`, the selection-indicator width is `0.0`, and the
transition duration is `180` milliseconds.

- [x] **Step 2: Add the shell contract test**

Replace the `3.0` indicator expectation with `0.0` and assert that
`ACTIVITY_ITEM_SIZE == ACTIVITY_RAIL_WIDTH == 48.0`.

- [x] **Step 3: Run the tests and confirm the expected failure**

Run:

```bash
cargo test -p strukt-ui --test component_contracts
cargo test -p strukt-app --test visual_foundation_contract
```

Expected: compilation or assertion failure because vector geometry and the new
rail constants do not exist yet.

### Task 3: Implement the canonical vector renderer

**Files:**
- Modify: `crates/strukt-ui/src/icon.rs`
- Modify: `crates/strukt-ui/src/lib.rs`

- [x] **Step 1: Replace Unicode glyph lookup with vector commands**

Define normalized 20-by-20 paths for every current `Icon` variant using a small
internal command enum (`Move`, `Line`, `Circle`, `Rectangle`, `Close`). Expose a
read-only `geometry()` contract for tests and remove `glyph()`.

- [x] **Step 2: Add widget-local transition state**

Implement an `IconCanvasState` containing current progress, target selection,
and the prior redraw instant. On `RedrawRequested`, move progress toward `0.0` or
`1.0` over `ACTIVITY_ICON_TRANSITION_MS`; request the next frame until complete.
When reduced motion is requested, assign the target progress immediately.

- [x] **Step 3: Draw the flat and 2.5D states from identical paths**

At progress `0.0`, render one muted 1.5-pixel face. Toward progress `1.0`, apply
a shallow top-down scale/rotation and draw two hard translated depth strokes, the
primary face, and a fine accent highlight from the exact same paths. Do not draw
blur, glow, or soft shadows.

- [x] **Step 4: Export the renderer and constants**

Export `icon_view`, `ACTIVITY_ICON_TRANSITION_MS`, and the geometry contract
needed by integration tests from `strukt-ui`.

### Task 4: Integrate the renderer into the activity rail

**Files:**
- Modify: `crates/strukt-ui/src/components/button.rs`
- Modify: `crates/strukt-ui/src/components/mod.rs`
- Modify: `crates/strukt-app/src/view/activity.rs`

- [x] **Step 1: Remove the side-indicator layout**

Set `ACTIVITY_SELECTION_INDICATOR_WIDTH` to `0.0`, introduce
`ACTIVITY_ITEM_SIZE: f32 = 48.0`, and render one full-width button rather than an
indicator-plus-control row.

- [x] **Step 2: Apply the edge-to-edge selected field**

Give selected buttons the semantic `panel_active` background, square corners,
no border, and the semantic primary text color. Keep inactive items transparent
and preserve existing hover, press, disabled, tooltip, keyboard, and message
behavior.

- [x] **Step 3: Replace the Unicode child with `icon_view`**

Render the canvas icon as the button content, passing selected state and the
current theme. Keep ordinary labeled icon buttons functional by composing the
same flat vector renderer next to their visible label.

- [x] **Step 4: Make rail items truly edge-to-edge**

Remove horizontal activity-column padding and inter-item spacing while retaining
the existing top inset and bottom-aligned Settings item.

### Task 5: Make tests pass and document verification

**Files:**
- Modify: `crates/strukt-ui/tests/component_contracts.rs`
- Modify: `crates/strukt-app/tests/visual_foundation_contract.rs`
- Modify: `docs/evidence/m5-5-visual-fidelity-validation.md`
- Modify: `docs/tracker.md`

- [x] **Step 1: Run focused tests**

Run:

```bash
cargo test -p strukt-ui --test component_contracts
cargo test -p strukt-app --test visual_foundation_contract
```

Expected: both test binaries pass.

- [x] **Step 2: Run formatting, lint, and M5.5 gates**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check-ui-semantics.sh
bash scripts/m5-5-visual-foundation-smoke.sh
cargo test --workspace
forj check .
```

Expected: every command exits zero; the smoke ends with
`M5.5 visual foundation smoke passed`.

- [ ] **Step 3: Perform native visual review**

Run `cargo run -p strukt-app`, select every activity in dark and light themes,
and record that the square reaches both rail edges, no accent bar remains, every
icon retains one silhouette, and the selected state reads as shallow top-down
2.5D at actual size.

- [x] **Step 4: Update evidence and tracker**

Record commands, platform, commit, visual findings, and any remaining deltas in
the M5.5 evidence document. Mark this plan complete only after the automated and
native checks pass; keep M5.5 `In review` until the milestone-wide acceptance
gate is satisfied.

## Self-review

- Spec coverage: geometry, icon-family continuity, 2.5D rendering, motion,
  reduced motion, accessibility, theming, and visual evidence all map to tasks.
- Placeholder scan: no deferred or ambiguous implementation steps remain.
- Type consistency: constants and renderer names are consistent across tasks.
