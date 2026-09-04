# M5.5 Mock Fidelity Correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the native `strukt` shell materially match the approved Quiet Precision promoteable-drawer mock while preserving all existing M1–M5 behavior.

**Architecture:** Keep product behavior in existing feature modules. Correct the visual system at three boundaries: exact built-in tokens and metrics in `strukt-theme`, complete styled primitives in `strukt-ui`, and compact edge-to-edge composition in `strukt-app`. Visual review uses deterministic reference-state metadata plus native screenshots; behavioral tests remain the regression authority.

**Tech Stack:** Rust 1.97.1, Iced 0.14, existing `strukt-theme`/`strukt-ui`/`strukt-app` crates, shell smoke scripts, native macOS screenshot review.

**Approved spec:** [`../specs/0010-m5-5-visual-and-interaction-foundation.md`](../specs/0010-m5-5-visual-and-interaction-foundation.md)

**North-star mock:** [`../mockups/visual-foundation/quiet-precision-spatial-policy.html`](../mockups/visual-foundation/quiet-precision-spatial-policy.html), option A

---

## File ownership map

- `crates/strukt-theme/src/tokens.rs`: exact dark/light semantic colors.
- `crates/strukt-theme/src/definition.rs`: compact default metrics shared by built-ins and future custom themes.
- `crates/strukt-ui/src/theme.rs`: state-to-token appearance resolution.
- `crates/strukt-ui/src/components/button.rs`: quiet, icon, activity, and primary button geometry.
- `crates/strukt-ui/src/components/chrome.rs`: contiguous chrome, headers, badges, dividers.
- `crates/strukt-ui/src/components/list.rs`: flat compact rows and selected indicators.
- `crates/strukt-ui/tests/component_contracts.rs`: visual primitive contracts.
- `crates/strukt-app/src/view/shell.rs`: reference shell dimensions and edge-to-edge canvas.
- `crates/strukt-app/src/view/activity.rs`: icon-first rail.
- `crates/strukt-app/src/view/mod.rs`: compact explorer and removal of global canvas padding.
- `crates/strukt-app/src/view/editor.rs`: editor surface routing only.
- `crates/strukt-app/src/view/terminal.rs`: drawer surface routing only.
- `crates/strukt-app/tests/visual_foundation_contract.rs`: measurable native composition contracts.
- `scripts/check-ui-semantics.sh`: prevent default-widget styling from leaking into application-owned chrome.
- `docs/evidence/m5-5-visual-fidelity-validation.md`: screenshot matrix and review findings.
- `README.md`, `docs/tracker.md`, `docs/roadmap.md`: milestone state.

### Task 1: Lock the north-star tokens and geometry

**Files:**
- Modify: `crates/strukt-theme/src/tokens.rs`
- Modify: `crates/strukt-theme/src/definition.rs`
- Modify: `crates/strukt-theme/tests/builtin_themes.rs`

- [ ] **Step 1: Write failing exact-token and metric assertions**

Add assertions for the dark baseline:

```rust
assert_eq!(tokens.canvas, Rgb::new(18, 21, 24));
assert_eq!(tokens.panel, Rgb::new(23, 26, 29));
assert_eq!(tokens.panel_active, Rgb::new(34, 39, 43));
assert_eq!(tokens.border, Rgb::new(48, 54, 59));
assert_eq!(tokens.text_primary, Rgb::new(232, 235, 237));
assert_eq!(tokens.text_muted, Rgb::new(146, 154, 161));
assert_eq!(tokens.accent, Rgb::new(128, 183, 170));
assert_eq!(definition.metrics.sidebar_width, 218.0);
assert_eq!(definition.metrics.context_width, 235.0);
assert_eq!(definition.metrics.drawer_height, 205.0);
assert_eq!(definition.metrics.row_height, 27.0);
```

- [ ] **Step 2: Run the focused test and confirm failure**

Run: `cargo test -p strukt-theme --test builtin_themes --locked --offline`

Expected: FAIL because the current GitHub-like palette and larger metrics differ.

- [ ] **Step 3: Set the built-in palette and metrics from the approved mock**

Update `quiet_precision_tokens(ThemeMode::Dark)` and `ThemeMetricsV1::quiet_precision()` with the asserted values. Keep the accessible light variant semantically equivalent and within existing contrast validation.

- [ ] **Step 4: Run theme tests**

Run: `cargo test -p strukt-theme --locked --offline`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/strukt-theme/src/tokens.rs crates/strukt-theme/src/definition.rs crates/strukt-theme/tests/builtin_themes.rs
git commit -m "fix(theme): align Quiet Precision with approved mock"
```

### Task 2: Make shared controls genuinely quiet

**Files:**
- Modify: `crates/strukt-ui/src/theme.rs`
- Modify: `crates/strukt-ui/src/components/button.rs`
- Modify: `crates/strukt-ui/src/components/list.rs`
- Modify: `crates/strukt-ui/tests/component_contracts.rs`

- [ ] **Step 1: Write failing appearance tests**

Assert that resting quiet controls use the surrounding panel color with a transparent border, selected list rows use `panel_active`, and activity selection uses `accent` only as its indicator/focus color.

```rust
let resting = button_appearance(&theme, Emphasis::Quiet, ComponentState::Resting);
assert_eq!(resting.background, semantic_color(theme.tokens.panel));
assert_eq!(resting.border, Color::TRANSPARENT);

let selected = list_row_appearance(&theme, SelectionState::Selected);
assert_eq!(selected.background, semantic_color(theme.tokens.panel_active));
assert_eq!(selected.focus, semantic_color(theme.tokens.accent));
```

- [ ] **Step 2: Run the focused test and confirm failure**

Run: `cargo test -p strukt-ui --test component_contracts --locked --offline`

Expected: FAIL because resting quiet buttons currently draw visible borders.

- [ ] **Step 3: Implement flat resting states and compact geometry**

Keep 28-pixel activity targets inside the accessible 40-pixel hit region, remove resting borders, use 2–3 pixel radii, and reserve filled accent backgrounds for `Emphasis::Strong` only.

- [ ] **Step 4: Run UI tests**

Run: `cargo test -p strukt-ui --locked --offline`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/strukt-ui/src/theme.rs crates/strukt-ui/src/components/button.rs crates/strukt-ui/src/components/list.rs crates/strukt-ui/tests/component_contracts.rs
git commit -m "fix(ui): restore restrained Quiet Precision controls"
```

### Task 3: Match the shell frame and explorer

**Files:**
- Modify: `crates/strukt-app/src/view/shell.rs`
- Modify: `crates/strukt-app/src/view/activity.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/tests/visual_foundation_contract.rs`

- [ ] **Step 1: Add failing geometry and composition tests**

Expose test-only constants and assert:

```rust
assert!((WORKSPACE_BAR_HEIGHT - 40.0).abs() <= 2.0);
assert!((ACTIVITY_RAIL_WIDTH - 48.0).abs() <= 2.0);
assert_eq!(STATUS_STRIP_HEIGHT, 25.0);
assert_eq!(CANVAS_OUTER_PADDING, 0.0);
assert_eq!(EXPLORER_PERMANENT_ACTION_ROWS, 0);
```

Also assert that the wide local reference composition contains rail, explorer, editor, drawer, optional context, and status in that order.

- [ ] **Step 2: Run the contract test and confirm failure**

Run: `cargo test -p strukt-app --test visual_foundation_contract --locked --offline`

Expected: FAIL on global canvas padding and permanent explorer actions.

- [ ] **Step 3: Implement compact shell geometry**

Make the workspace bar 40 pixels, rail 48 pixels, status 25 pixels, default sidebar 218 pixels, and context 235 pixels. Remove the 20-pixel primary-canvas wrapper padding. Keep padding only inside workflow-specific empty states and forms.

- [ ] **Step 4: Simplify the explorer**

Render one 41-pixel header with `EXPLORER`, add, and overflow affordances. Render a flat 27-pixel tree below it. Move open-folder, visibility, create, rename, duplicate, and delete actions to the empty state, command center, header actions, or existing contextual dialog paths instead of permanent action rows.

- [ ] **Step 5: Run app contract and regression tests**

Run: `cargo test -p strukt-app --locked --offline`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/strukt-app/src/view/shell.rs crates/strukt-app/src/view/activity.rs crates/strukt-app/src/view/mod.rs crates/strukt-app/tests/visual_foundation_contract.rs
git commit -m "fix(app): match the Quiet Precision shell frame"
```

### Task 4: Make editor and terminal contiguous surfaces

**Files:**
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/view/editor.rs`
- Modify: `crates/strukt-app/src/view/terminal.rs`
- Modify: `crates/strukt-app/tests/visual_foundation_contract.rs`

- [ ] **Step 1: Add failing surface-contract tests**

Assert 33-pixel editor tabs, 28-pixel breadcrumbs, a 33-pixel drawer header, no inter-tab spacing, and no always-visible editor command toolbar.

- [ ] **Step 2: Run the focused test and confirm failure**

Run: `cargo test -p strukt-app --test visual_foundation_contract --locked --offline`

Expected: FAIL because tabs currently have gaps and the editor always renders a large command row.

- [ ] **Step 3: Recompose the editor**

Render contiguous tabs, one breadcrumb strip, and the editor filling the remaining canvas. Move save, undo, redo, find, language actions, and language selection into keyboard commands, the command center, or a compact on-demand overflow surface while preserving their existing messages and behavior.

- [ ] **Step 4: Recompose the terminal drawer**

Render a 33-pixel header, a contiguous terminal body, compact cycle/promote/close actions, and a 205-pixel default height. Preserve terminal identity when closing, reopening, splitting, or promoting.

- [ ] **Step 5: Run editor, terminal, and app tests**

Run: `cargo test -p strukt-app --locked --offline`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/strukt-app/src/view/mod.rs crates/strukt-app/src/view/editor.rs crates/strukt-app/src/view/terminal.rs crates/strukt-app/tests/visual_foundation_contract.rs
git commit -m "fix(app): make editor and terminal match the north star"
```

### Task 5: Eliminate default-widget visual leakage

**Files:**
- Modify: `scripts/check-ui-semantics.sh`
- Modify: `crates/strukt-ui/src/components/mod.rs`
- Create: `crates/strukt-ui/src/components/input.rs`
- Create: `crates/strukt-ui/src/components/tab.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/view/command_center.rs`
- Modify: `crates/strukt-app/src/view/connections.rs`
- Modify: `crates/strukt-app/src/view/search.rs`
- Modify: `crates/strukt-app/src/view/sessions.rs`
- Modify: `crates/strukt-app/src/view/settings.rs`

- [ ] **Step 1: Extend the semantic guard and confirm failure**

Reject unstyled `button(`, `pick_list(`, and `text_input(` calls in application-owned persistent chrome and require an explicit styling function or a `strukt_ui` primitive.

Run: `bash scripts/check-ui-semantics.sh`

Expected: FAIL and list remaining leaks.

- [ ] **Step 2: Add only the missing shared primitives**

Implement compact text inputs, pick lists, tabs, and toolbar actions using existing semantic roles. Do not move feature behavior into `strukt-ui`.

- [ ] **Step 3: Migrate persistent chrome and rerun the guard**

Run: `bash scripts/check-ui-semantics.sh`

Expected: PASS with no raw default widget in persistent shell, explorer, editor chrome, drawer chrome, context, or status.

- [ ] **Step 4: Run full Rust verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo test --workspace --locked --offline
```

Expected: all commands exit 0.

- [ ] **Step 5: Commit**

```bash
git add scripts/check-ui-semantics.sh crates/strukt-ui/src crates/strukt-app/src/view
git commit -m "fix(ui): prevent default widget style leakage"
```

### Task 6: Iterate against native visual evidence

**Files:**
- Create: `docs/evidence/m5-5-visual-fidelity-validation.md`
- Modify: `crates/strukt-theme/src/tokens.rs`
- Modify: `crates/strukt-theme/src/definition.rs`
- Modify: `crates/strukt-ui/src/theme.rs`
- Modify: `crates/strukt-ui/src/components/button.rs`
- Modify: `crates/strukt-ui/src/components/chrome.rs`
- Modify: `crates/strukt-ui/src/components/list.rs`
- Modify: `crates/strukt-ui/src/components/input.rs`
- Modify: `crates/strukt-ui/src/components/tab.rs`
- Modify: `crates/strukt-app/src/view/shell.rs`
- Modify: `crates/strukt-app/src/view/activity.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`

- [ ] **Step 1: Build and launch the reference state**

Run: `cargo run -p strukt-app`

Open the repository workspace, open two documents, open a local terminal drawer, and show context at a wide viewport.

- [ ] **Step 2: Capture the required native matrix**

Capture dark wide, light wide, compact, drawer closed, drawer open, split terminal, local boundary, and remote boundary screenshots. Record platform, viewport, and commit in the evidence document.

- [ ] **Step 3: Compare each capture with option A**

Review geometry, hierarchy, spacing, typography, borders, color emphasis, and control density. Record every material mismatch. Fix one visual category at a time and recapture until no material mismatch remains.

- [ ] **Step 4: Run the full milestone gate**

```bash
forj check .
bash scripts/check-ui-semantics.sh
bash scripts/m5-5-visual-foundation-smoke.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo test --workspace --locked --offline
cargo build --workspace --locked --offline
git diff --check
```

Expected: all commands exit 0, and the evidence document has no unresolved material visual delta.

- [ ] **Step 5: Commit**

```bash
git add docs/evidence/m5-5-visual-fidelity-validation.md crates/strukt-theme crates/strukt-ui crates/strukt-app scripts/check-ui-semantics.sh
git commit -m "test: record M5.5 native visual fidelity"
```

### Task 7: Restore review readiness

**Files:**
- Modify: `README.md`
- Modify: `docs/tracker.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/plans/0011-m5-5-mock-fidelity-correction.md`

- [ ] **Step 1: Update milestone status and evidence links**

Mark every completed task, link native visual evidence, and state that M5.5 is review-ready only after the screenshot matrix and automated gate pass.

- [ ] **Step 2: Validate documentation**

```bash
forj check docs/specs/0010-m5-5-visual-and-interaction-foundation.md
forj check docs/plans/0011-m5-5-mock-fidelity-correction.md
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 3: Commit**

```bash
git add README.md docs/tracker.md docs/roadmap.md docs/plans/0011-m5-5-mock-fidelity-correction.md
git commit -m "docs: restore M5.5 visual review readiness"
```
