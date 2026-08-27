# M5.5 Visual and Interaction Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the prototype presentation of every M1–M5 workflow with the approved Quiet Precision visual system, adaptive workspace shell, reusable native components, and versioned dual-theme contract without adding product capabilities.

**Architecture:** Keep product behavior in the existing feature crates. Extend `strukt-theme` with a UI-independent, validated theme-definition boundary; add an Iced-dependent `strukt-ui` component crate; and make `strukt-shell` own composition, navigation contributions, commands, focus, and promoteable-drawer state. `strukt-app` remains the composition root and migrates each existing surface onto those contracts in bounded delivery slices.

**Tech Stack:** Rust 1.97.1, Iced 0.14 (`wgpu`, `advanced`, `canvas`, `highlighter`, `tokio`), Serde/JSON, existing workspace crates and shell smoke scripts, GitHub Actions on macOS/Windows/Ubuntu.

**Status:** Approved — implementation in progress

**Approved spec:** [`../specs/0010-m5-5-visual-and-interaction-foundation.md`](../specs/0010-m5-5-visual-and-interaction-foundation.md)

---

## Delivery model

M5.5 is one milestone with one umbrella tracking issue and five ordered pull-request slices. Every slice links the approved spec, this plan, and the umbrella issue. A slice may merge only after its own tests and existing regression tests pass; M5.5 remains incomplete until Slice 5 closes the final acceptance gate.

1. **Foundation:** versioned themes, `strukt-ui`, and shell composition contracts.
2. **Shell:** Quiet Precision chrome, command center, persistence, and drawer mechanics.
3. **Local workflows:** files, search, Git, editor, language context, problems, and terminal.
4. **Session workflows:** local sessions, remote connections, remote workspaces, and persistent remote sessions.
5. **Release gate:** accessibility, responsive behavior, deterministic smoke coverage, cross-platform evidence, documentation, and agentic review.

The work is sequential at the slice level because later views consume earlier contracts. Within a slice, tests are written before implementation and commits remain small enough to review independently.

## Global constraints

- Do not add AI, plugin loading, MCP discovery, new terminal behavior, new SSH behavior, external theme discovery, packaging, or release publication.
- Do not move domain behavior from feature crates into `strukt-app`, `strukt-shell`, or `strukt-ui`.
- Do not let feature views define raw chrome colors. All chrome uses semantic roles resolved by `strukt-theme` and consumed by `strukt-ui`.
- Preserve terminal runtime identity while changing presentation or promotion state.
- Persist composition identifiers and bounded geometry only; never persist terminal output, file contents, secrets, or remote credentials.
- Use `Cmd` on macOS and `Ctrl` on Windows/Linux for the same logical shortcuts.
- Keep dark and light variants at parity in every acceptance check.
- Keep the current `.cargo/config.toml` platform linker behavior and existing offline verification path.

## Slice 1 — Foundation contracts

### Task 1: Establish delivery tracking

**Files:**
- Modify: `README.md`
- Modify: `docs/tracker.md`
- Modify: `docs/roadmap.md`
- Create through GitHub: one M5.5 umbrella issue

- [x] **Step 1: Create the umbrella issue from the approved scope**

Run:

```bash
gh issue create --title "M5.5: Visual and Interaction Foundation" --body-file docs/specs/0010-m5-5-visual-and-interaction-foundation.md
```

Expected: GitHub prints the new issue URL. Record its number as `M5_5_ISSUE` for the remaining delivery slices.

- [x] **Step 2: Update roadmap and tracker links**

Change M5.5 from `Shaping` to `Planned`, link this plan, and link the umbrella issue. Leave PR and evidence columns empty until those artifacts exist.

- [x] **Step 3: Validate documentation**

Run:

```bash
forj check docs/specs/0010-m5-5-visual-and-interaction-foundation.md
forj check docs/plans/0010-m5-5-visual-and-interaction-foundation.md
git diff --check
```

Expected: both `forj check` commands and `git diff --check` exit 0.

- [x] **Step 4: Commit the tracking transition**

```bash
git add README.md docs/plans/0010-m5-5-visual-and-interaction-foundation.md docs/tracker.md docs/roadmap.md
git commit -m "docs: start M5.5 implementation"
```

### Task 2: Define and validate the versioned theme contract

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/strukt-theme/Cargo.toml`
- Modify: `crates/strukt-theme/src/lib.rs`
- Modify: `crates/strukt-theme/src/tokens.rs`
- Create: `crates/strukt-theme/src/definition.rs`
- Create: `crates/strukt-theme/src/registry.rs`
- Create: `crates/strukt-theme/src/validation.rs`
- Create: `crates/strukt-theme/tests/theme_definition.rs`
- Create: `crates/strukt-theme/tests/theme_validation.rs`
- Modify: `crates/strukt-theme/tests/builtin_themes.rs`

- [x] **Step 1: Write failing serialization and resolution tests**

Cover this public contract:

```rust
pub const THEME_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThemeId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeDefinitionV1 {
    pub schema_version: u16,
    pub id: ThemeId,
    pub display_name: String,
    pub author: String,
    pub variants: BTreeMap<ThemeMode, ThemeVariantV1>,
    pub metrics: ThemeMetricsV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeVariantV1 {
    pub palette: BTreeMap<String, Rgb>,
    pub roles: BTreeMap<ThemeRole, String>,
}
```

Tests must assert JSON round-trip equality, both required variants, stable snake-case role names, and resolution from palette keys into a complete `ThemeTokens` value.

- [x] **Step 2: Confirm the tests fail for the missing contract**

```bash
cargo test -p strukt-theme --test theme_definition --locked --offline
```

Expected: compilation fails because the new contract types are not exported.

- [x] **Step 3: Implement the definition and resolver**

Add every current `ThemeTokens` field to `ThemeRole`, including the sixteen ANSI slots. Add `ThemeRole::ALL` and resolve roles without silently substituting values. `ThemeMetricsV1` must contain only visual metrics:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeMetricsV1 {
    pub space_1: f32,
    pub space_2: f32,
    pub space_3: f32,
    pub space_4: f32,
    pub radius_small: f32,
    pub radius_medium: f32,
    pub control_height: f32,
    pub row_height: f32,
    pub sidebar_width: f32,
    pub context_width: f32,
    pub drawer_height: f32,
}
```

Reject missing palette references, missing roles, unsupported schema versions, invalid identifiers, non-finite metrics, and metrics outside the bounds documented in the spec.
Extend `ThemeMode` with Serde and total-order derives so it is a stable serialized `BTreeMap` key.

- [x] **Step 4: Write failing validation and fallback tests**

Test unsupported schema versions, missing dark or light variants, bad role references, incomplete roles, contrast failures, and registry fallback to Quiet Precision when a selected ID is absent or invalid.

- [x] **Step 5: Implement validation and registry fallback**

Expose:

```rust
pub struct ThemeRegistry {
    definitions: BTreeMap<ThemeId, ThemeDefinitionV1>,
    fallback: ThemeId,
}

impl ThemeRegistry {
    pub fn with_builtins() -> Self;
    pub fn register(&mut self, definition: ThemeDefinitionV1) -> Result<(), ThemeValidationError>;
    pub fn resolve(&self, id: &ThemeId, mode: ThemeMode) -> ResolvedTheme;
}
```

Validate text-on-canvas and text-on-panel at 4.5:1, muted text at 3:1, and focus/error indicators at 3:1. Keep validation UI-independent and deterministic.

- [x] **Step 6: Express both built-ins through the same definition contract**

Replace direct `ThemeTokens::builtin` construction with one built-in `ThemeDefinitionV1` named `quiet-precision` containing complete light and dark variants. Preserve `ThemeTokens::builtin(mode)` as a compatibility wrapper that resolves that built-in definition.

- [x] **Step 7: Run theme verification**

```bash
cargo fmt --all -- --check
cargo clippy -p strukt-theme --all-targets --all-features --locked --offline -- -D warnings
cargo test -p strukt-theme --all-targets --locked --offline
```

Expected: all commands exit 0 and tests cover every semantic role in both variants.

- [x] **Step 8: Commit the theme contract**

```bash
git add Cargo.toml crates/strukt-theme
git commit -m "feat(theme): add versioned semantic theme definitions"
```

### Task 3: Add the reusable Iced component crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/strukt-ui/Cargo.toml`
- Create: `crates/strukt-ui/src/lib.rs`
- Create: `crates/strukt-ui/src/theme.rs`
- Create: `crates/strukt-ui/src/icon.rs`
- Create: `crates/strukt-ui/src/components/mod.rs`
- Create: `crates/strukt-ui/src/components/button.rs`
- Create: `crates/strukt-ui/src/components/chrome.rs`
- Create: `crates/strukt-ui/src/components/list.rs`
- Create: `crates/strukt-ui/src/components/state.rs`
- Create: `crates/strukt-ui/tests/component_contracts.rs`

- [x] **Step 1: Add a failing component-contract test**

The test must instantiate dark and light `UiTheme` values and assert that default, hovered, focused, selected, disabled, warning, and error appearances resolve exclusively from semantic roles.

- [x] **Step 2: Confirm the new package does not exist**

```bash
cargo test -p strukt-ui --all-targets --locked --offline
```

Expected: Cargo reports that package `strukt-ui` is missing.

- [x] **Step 3: Add the crate and public visual contract**

Add `crates/strukt-ui` to workspace members and dependencies. Its dependency surface is `iced`, `strukt-theme`, and `thiserror`. Export:

```rust
#[derive(Clone, Debug)]
pub struct UiTheme {
    pub tokens: ThemeTokens,
    pub metrics: ThemeMetricsV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emphasis {
    Quiet,
    Standard,
    Strong,
    Destructive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionState {
    Resting,
    Hovered,
    Focused,
    Selected,
    Disabled,
}
```

- [x] **Step 4: Implement the first component set**

Provide semantic builders for icon buttons, text buttons, activity-rail items, panel headers, toolbar groups, list rows, badges, dividers, empty states, error states, and loading states. Icons use one 16-pixel stroke family and must include accessible text labels at the call site; do not introduce an icon font or image dependency.

- [x] **Step 5: Run component verification**

```bash
cargo fmt --all -- --check
cargo clippy -p strukt-ui --all-targets --all-features --locked --offline -- -D warnings
cargo test -p strukt-ui --all-targets --locked --offline
```

Expected: all commands exit 0 in both theme modes.

- [x] **Step 6: Commit the component foundation**

```bash
git add Cargo.toml crates/strukt-ui
git commit -m "feat(ui): add quiet precision component foundation"
```

### Task 4: Replace toggle booleans with explicit shell composition state

**Files:**
- Modify: `crates/strukt-shell/src/lib.rs`
- Modify: `crates/strukt-shell/src/state.rs`
- Create: `crates/strukt-shell/src/activity.rs`
- Create: `crates/strukt-shell/src/composition.rs`
- Create: `crates/strukt-shell/src/contribution.rs`
- Create: `crates/strukt-shell/tests/composition_state.rs`
- Modify: `crates/strukt-shell/tests/shell_state.rs`

- [x] **Step 1: Write failing composition transition tests**

Cover: changing activities updates the contextual sidebar and canvas owner; opening a tool in the drawer does not replace the canvas; promoting a drawer tool to split or full preserves its `SurfaceId`; demoting restores the prior canvas; hiding a focused panel returns focus to the canvas; removing a contribution removes all of its navigation and surface references; invalid ratios are clamped.

- [x] **Step 2: Confirm the new tests fail**

```bash
cargo test -p strukt-shell --test composition_state --locked --offline
```

Expected: compilation fails because explicit composition types do not exist.

- [x] **Step 3: Implement UI-independent composition types**

Use stable identifiers and explicit placement:

```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum CanvasLayout {
    Single { primary: SurfaceId },
    Split { primary: SurfaceId, secondary: SurfaceId, ratio: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawerState {
    pub surface: Option<SurfaceId>,
    pub visible: bool,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusRegion {
    ActivityRail,
    Sidebar,
    Canvas,
    ContextPanel,
    Drawer,
    CommandCenter,
}
```

`ShellState` owns the active activity, optional sidebar/context visibility, canvas layout, drawer state, focus region, and theme selection. Feature-specific runtime state remains outside this crate.

- [x] **Step 4: Add feature contribution contracts**

```rust
pub struct ShellContribution {
    pub id: String,
    pub activity: Option<ActivityContribution>,
    pub surfaces: Vec<SurfaceContribution>,
    pub commands: Vec<CommandContribution>,
}
```

Registration must reject duplicate contribution, activity, surface, and command IDs. Unregistration must atomically sanitize shell state and select the Files contribution or first available contribution as fallback.

- [x] **Step 5: Preserve compatibility actions while migrating callers**

Keep current public `Activity` variants and translate old `ToggleExplorer`, `ToggleContext`, and `ToggleDrawer` actions into explicit transitions. Mark compatibility paths with Rust deprecation attributes only after all app callers migrate in Slice 2.

- [x] **Step 6: Verify shell state**

```bash
cargo fmt --all -- --check
cargo clippy -p strukt-shell --all-targets --all-features --locked --offline -- -D warnings
cargo test -p strukt-shell --all-targets --locked --offline
```

Expected: all existing shell tests and new transition tests pass.

- [x] **Step 7: Commit composition state**

```bash
git add crates/strukt-shell
git commit -m "feat(shell): model adaptive workspace composition"
```

## Slice 2 — Adaptive shell

### Task 5: Persist bounded shell composition safely

**Files:**
- Modify: `crates/strukt-persistence/src/lib.rs`
- Create: `crates/strukt-persistence/src/shell_store.rs`
- Create: `crates/strukt-persistence/tests/shell_store.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/workspace.rs`

- [x] **Step 1: Write failing shell snapshot tests**

Test round-trip persistence, schema rejection, width/height/ratio clamping, missing-surface fallback, corrupt JSON fallback, and preservation of unrelated workspace contributions.

- [x] **Step 2: Confirm tests fail for the missing store**

```bash
cargo test -p strukt-persistence --test shell_store --locked --offline
```

Expected: compilation fails because `shell_store` is not exported.

- [x] **Step 3: Implement the bounded schema**

```rust
pub const SHELL_CONTRIBUTION_ID: &str = "shell";
pub const SHELL_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellSnapshotV1 {
    pub schema_version: u16,
    pub active_activity: String,
    pub sidebar_visible: bool,
    pub sidebar_width: u16,
    pub context_visible: bool,
    pub context_width: u16,
    pub canvas: PersistedCanvasLayout,
    pub drawer_surface: Option<String>,
    pub drawer_visible: bool,
    pub drawer_height: u16,
    pub theme_id: String,
    pub theme_mode: ThemeMode,
}
```

Clamp sidebar and context widths to `180..=640`, drawer height to `120..=720`, and split ratio to `0.2..=0.8`. Store IDs and geometry only. Return a structured warning and default snapshot for invalid data instead of failing workspace startup.

- [x] **Step 4: Wire restore and save through the existing contribution map**

On workspace load, restore shell composition after contributions register so missing IDs can be sanitized. On each accepted shell transition, update only `WorkspaceState.contributions[SHELL_CONTRIBUTION_ID]`; preserve every sibling key byte-for-byte through the Serde value boundary.

- [x] **Step 5: Verify persistence and app regressions**

```bash
cargo test -p strukt-persistence --all-targets --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
```

Expected: shell persistence tests and all existing app tests pass.

- [x] **Step 6: Commit shell persistence**

```bash
git add crates/strukt-persistence crates/strukt-app/src/app.rs crates/strukt-app/src/workspace.rs
git commit -m "feat(shell): persist workspace composition"
```

### Task 6: Add the unified command-center model

**Files:**
- Modify: `crates/strukt-shell/src/lib.rs`
- Create: `crates/strukt-shell/src/command.rs`
- Create: `crates/strukt-shell/tests/command_center.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`

- [x] **Step 1: Write failing command model tests**

Test deterministic registration order, case-insensitive token matching, category filtering, disabled-command visibility, duplicate rejection, local/remote execution-boundary labels, and selection returning an ID without executing feature behavior.

- [x] **Step 2: Implement the command contract**

```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionBoundary {
    Local,
    Remote,
    Interface,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandContribution {
    pub id: CommandId,
    pub title: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub shortcut: Option<String>,
    pub boundary: ExecutionBoundary,
    pub enabled: bool,
}
```

`CommandCatalog::search(query)` returns ranked contributions. `strukt-shell` never invokes feature code; `strukt-app` maps the selected `CommandId` to existing messages.

- [x] **Step 3: Register all existing global actions**

Register activity navigation, open folder, file search, source control, session views, connection views, context toggle, drawer toggle, terminal drawer, theme mode, settings, and every existing keyboard-reachable action. Show `LOCAL`, remote host alias, or `INTERFACE` before execution.

- [x] **Step 4: Verify command behavior**

```bash
cargo test -p strukt-shell --test command_center --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
```

Expected: all tests pass and command selection is covered without spawning a process or remote request.

- [x] **Step 5: Commit the command model**

```bash
git add crates/strukt-shell crates/strukt-app/src/main.rs crates/strukt-app/src/app.rs
git commit -m "feat(shell): add unified command center"
```

### Task 7: Build the Quiet Precision shell chrome

**Files:**
- Modify: `crates/strukt-app/Cargo.toml`
- Move: `crates/strukt-app/src/view.rs` to `crates/strukt-app/src/view/mod.rs`
- Create: `crates/strukt-app/src/view/shell.rs`
- Create: `crates/strukt-app/src/view/activity.rs`
- Create: `crates/strukt-app/src/view/command_center.rs`
- Create: `crates/strukt-app/src/view/status.rs`
- Create: `crates/strukt-app/src/view/state.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`

- [x] **Step 1: Add failing shell-view state tests**

Test that only the active activity is selected; sidebar/context/drawer presence follows composition state; command center traps focus while open; Escape closes the top overlay; closing a panel returns focus; and remote boundaries appear in the title/status chrome.

- [x] **Step 2: Perform the mechanical module move**

Move `view.rs` to `view/mod.rs` without behavior changes, run `cargo fmt`, and prove the app tests remain green before extracting submodules.

```bash
cargo test -p strukt-app --all-targets --locked --offline
```

Expected: the pre-redesign app tests pass after the move.

- [x] **Step 3: Add `strukt-ui` to the app and replace top-level chrome**

Build the stable activity rail, contextual sidebar slot, adaptive canvas slot, on-demand context slot, promoteable drawer slot, and compact status strip using `strukt-ui`. Remove the current header full of equal-weight accent buttons. Keep the platform title bar native and use low-radius, low-noise controls.

- [x] **Step 4: Implement the command-center overlay**

Open it with logical `Primary+K`; search commands as the user types; display category, shortcut, and execution boundary; dispatch only after confirmation; close on Escape or successful selection; return focus to the prior region.

- [x] **Step 5: Implement explicit state-language views**

Use shared empty, loading, recoverable error, unavailable, disconnected, and disabled presentations. Every recoverable failure includes one primary recovery action and optional detail disclosure; raw error strings do not become the visual hierarchy.

- [x] **Step 6: Verify shell chrome**

```bash
cargo fmt --all -- --check
cargo clippy -p strukt-app --all-targets --all-features --locked --offline -- -D warnings
cargo test -p strukt-app --all-targets --locked --offline
```

Expected: all shell state tests and existing app behavior tests pass.

- [x] **Step 7: Commit the shell chrome**

```bash
git add crates/strukt-app
git commit -m "feat(app): build adaptive quiet precision shell"
```

## Slice 3 — Local workflows

### Task 8: Migrate Files, Search, Git, and Settings surfaces

**Files:**
- Create: `crates/strukt-app/src/view/files.rs`
- Create: `crates/strukt-app/src/view/search.rs`
- Create: `crates/strukt-app/src/view/source_control.rs`
- Create: `crates/strukt-app/src/view/settings.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/workspace.rs`

- [x] **Step 1: Add failing composition tests for local activities**

For each activity, assert the sidebar title, canvas owner, available contextual actions, keyboard focus target, empty state, and one representative existing operation. File operations must retain confirmation and exclusion behavior.

- [x] **Step 2: Migrate Files**

Use a compact tree with quiet row selection, toolbar icons with labels, inline create/rename affordances, clear hidden/ignored filters, and confirmation for destructive actions. Opening a file owns the canvas; the Files tree stays contextual rather than becoming the canvas.

- [x] **Step 3: Migrate Search and Source Control**

Search uses a focused query field, filter disclosure, grouped results, and inline match context. Source Control uses grouped changes, explicit repository state, and restrained primary actions. Preserve existing Git behavior and no-op/disabled states.

- [x] **Step 4: Migrate Settings**

Settings owns the canvas, groups existing settings semantically, and exposes dark/light selection through theme ID plus mode. Do not add external-theme browsing or editing.

- [x] **Step 5: Run local surface regression tests**

```bash
cargo test -p strukt-fs --all-targets --locked --offline
cargo test -p strukt-workspace --all-targets --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
fixture="$(mktemp -d)"
printf 'strukt\n' > "$fixture/strukt-smoke.txt"
cargo run -p strukt-app --locked --offline -- --workspace-files-smoke "$fixture"
```

Expected: all commands exit 0 and existing file/search/Git/settings operations remain reachable.

- [x] **Step 6: Commit local navigation surfaces**

```bash
git add crates/strukt-app
git commit -m "feat(app): migrate local workspace surfaces"
```

### Task 9: Migrate editor, language context, and problems

**Files:**
- Create: `crates/strukt-app/src/view/editor.rs`
- Create: `crates/strukt-app/src/view/context.rs`
- Create: `crates/strukt-app/src/view/problems.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/editor.rs`
- Modify: `crates/strukt-app/src/language.rs`
- Modify: `crates/strukt-app/src/app.rs`

- [x] **Step 1: Add failing editor composition tests**

Cover active tab identity, modified indicator, save/conflict/recovery notices, syntax-theme parity, diagnostics counts, problem filtering, context-panel toggling, language-server unavailable state, and focus restoration.

- [x] **Step 2: Migrate editor canvas**

Make the document the dominant surface. Use a compact tab/title row, semantic modified/conflict state, quiet gutters, stable line metrics, and unobtrusive recovery notices. Preserve editing, selection, save, conflict, and recovery behavior exactly.

- [x] **Step 3: Migrate Problems and workspace context**

Problems opens as a supporting panel or promoted surface, never a permanently reserved column. Context is on demand, separates diagnostics from workspace metadata, and uses shared state language when no language server applies.

- [x] **Step 4: Verify editor and language behavior**

```bash
cargo test -p strukt-editor --all-targets --locked --offline
cargo test -p strukt-language --all-targets --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
fixture="$(mktemp -d)"
printf 'strukt\n' > "$fixture/strukt-smoke.txt"
printf 'strukt\n' > "$fixture/strukt-editor-smoke.txt"
cargo run -p strukt-app --locked --offline -- --editor-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --language-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --m2-integration-smoke "$fixture"
```

Expected: all commands exit 0 in both built-in theme modes.

- [ ] **Step 5: Commit editor and context migration**

```bash
git add crates/strukt-app
git commit -m "feat(app): migrate editor and language surfaces"
```

### Task 10: Migrate terminal into the promoteable drawer

**Files:**
- Create: `crates/strukt-app/src/view/terminal.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/terminal.rs`
- Modify: `crates/strukt-app/src/terminal_widget.rs`
- Modify: `crates/strukt-app/src/app.rs`

- [ ] **Step 1: Add failing terminal placement tests**

Open the same terminal surface in drawer, split, and full placements and assert that its runtime/session identifier, output buffer, input routing, current directory, and process lifecycle are unchanged. Assert that closing the visual placement does not terminate the runtime unless the user invokes the existing terminate action.

- [ ] **Step 2: Implement placement-independent terminal rendering**

Render the existing terminal model through a `SurfaceId`. Drawer controls expose promote-to-split, promote-to-full, demote, maximize/restore, and close-placement actions. Use terminal semantic colors and preserve the existing renderer and PTY behavior.

- [ ] **Step 3: Add keyboard and focus behavior**

The existing terminal shortcut opens/focuses the drawer. Promotion keeps terminal input focus. Escape follows the documented overlay/panel priority without being sent to the PTY only when the shell owns the key event.

- [ ] **Step 4: Verify terminal identity and regressions**

```bash
cargo test -p strukt-terminal --all-targets --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
fixture="$(mktemp -d)"
cargo run -p strukt-app --locked --offline -- --terminal-smoke "$fixture"
```

Expected: runtime identity tests pass and the M1 smoke exits 0.

- [ ] **Step 5: Commit terminal presentation**

```bash
git add crates/strukt-app
git commit -m "feat(app): add promoteable terminal surface"
```

## Slice 4 — Local and remote session workflows

### Task 11: Migrate local workspace sessions

**Files:**
- Create: `crates/strukt-app/src/view/sessions.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/session.rs`
- Modify: `crates/strukt-app/src/app.rs`

- [ ] **Step 1: Add failing session-view tests**

Test active/detached/exited states, empty state, create/attach/detach/rename/terminate confirmations, selected session focus, and stable session identity across view placement changes.

- [ ] **Step 2: Implement the compact session surface**

Use a dense semantic list in the contextual sidebar and session detail or attached terminal in the canvas. State chips use semantic session roles and text, not color alone. Destructive termination stays distinct from closing or detaching a view.

- [ ] **Step 3: Verify session behavior**

```bash
cargo test -p strukt-session --all-targets --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
fixture="$(mktemp -d)"
cargo run -p strukt-app --locked --offline -- --session-smoke "$fixture"
```

Expected: all commands exit 0 and all pre-existing session operations remain reachable.

- [ ] **Step 4: Commit local session migration**

```bash
git add crates/strukt-app
git commit -m "feat(app): migrate local session workflows"
```

### Task 12: Migrate remote connections, workspaces, and persistent sessions

**Files:**
- Create: `crates/strukt-app/src/view/connections.rs`
- Create: `crates/strukt-app/src/view/remote_workspace.rs`
- Modify: `crates/strukt-app/src/view/sessions.rs`
- Modify: `crates/strukt-app/src/view/mod.rs`
- Modify: `crates/strukt-app/src/remote.rs`
- Modify: `crates/strukt-app/src/app.rs`

- [ ] **Step 1: Add failing remote composition tests**

Cover connection list/search, host-key confirmation, authentication failure, helper unavailable/compatible/incompatible states, remote root selection, remote file opening, remote boundary labels, persistent-session list/attach/detach/create/rename/terminate, and local fallback after disconnection.

- [ ] **Step 2: Migrate connection entry and state language**

Use a focused connection canvas with compact saved-host rows and a clear primary action. Show host alias plus execution boundary in title/status chrome at all times. Authentication, host-key, network, helper, and permission errors use distinct recovery actions.

- [ ] **Step 3: Migrate remote workspace shell**

Reuse Files, Search, editor, Problems, context, and terminal components with remote providers. Keep capability checks visible and disable unsupported commands with reasons. Do not fork a second visual system for remote mode.

- [ ] **Step 4: Migrate persistent remote sessions**

Render remote tmux-compatible sessions through the shared session components while retaining remote provider semantics. Attaching a session opens its terminal as the same promoteable surface used locally. Execution boundary confirmation precedes destructive remote commands.

- [ ] **Step 5: Verify remote behavior**

```bash
cargo test -p strukt-remote --all-targets --locked --offline
cargo test -p strukt-session --all-targets --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
fixture="$(mktemp -d)"
cargo run -p strukt-app --locked --offline -- --remote-smoke "$fixture"
bash scripts/m5-remote-sessions-smoke.sh
```

Expected: all commands exit 0 without requiring a live remote host; integration fakes prove the UI dispatches existing remote operations.

- [ ] **Step 6: Perform a live remote walkthrough when a test host is available**

Record evidence for: connect, trust host key, choose remote root, browse/open/save a file, open terminal, create two persistent sessions, detach, reconnect, reattach each session, and disconnect safely. Redact hostnames, usernames, addresses, keys, and terminal content from committed evidence.

- [ ] **Step 7: Commit remote workflow migration**

```bash
git add crates/strukt-app
git commit -m "feat(app): migrate remote workspace workflows"
```

## Slice 5 — Product-quality gate

### Task 13: Enforce accessibility and responsive composition

**Files:**
- Modify: `crates/strukt-ui/src/components/button.rs`
- Modify: `crates/strukt-ui/src/components/chrome.rs`
- Modify: `crates/strukt-ui/src/components/list.rs`
- Modify: `crates/strukt-app/src/view/shell.rs`
- Modify: `crates/strukt-app/src/view/command_center.rs`
- Create: `crates/strukt-app/src/view/accessibility.rs`
- Create: `crates/strukt-app/src/view/responsive.rs`
- Create: `crates/strukt-app/tests/visual_foundation_contract.rs`

- [ ] **Step 1: Write failing accessibility and size-policy tests**

Test visible focus for every interactive component, text alternatives for icons, keyboard traversal order, focus trapping/restoration, semantic state text, minimum control target, reduced-motion selection, and deterministic composition at widths `960`, `1280`, and `1728` pixels.

- [ ] **Step 2: Implement platform-neutral logical shortcuts**

Centralize `Primary`, `Primary+K`, activity navigation, panel toggles, drawer focus, and Escape precedence. Map `Primary` to Command on macOS and Control on Windows/Linux. Do not scatter platform conditionals through feature views.

- [ ] **Step 3: Implement responsive policy**

At constrained width, collapse the context panel first, then the sidebar; preserve the canvas and activity rail; keep collapsed regions keyboard-reachable. At wide width, do not automatically open context. Clamp restored geometry using the persistence bounds.

- [ ] **Step 4: Implement reduced-motion behavior**

Keep all state transitions functional with animation duration zero. If the platform preference is unavailable through current dependencies, default to reduced motion and document the limitation; do not add a new system-integration dependency in M5.5.

- [ ] **Step 5: Verify accessibility contracts**

```bash
cargo test -p strukt-ui --all-targets --locked --offline
cargo test -p strukt-app --test visual_foundation_contract --locked --offline
cargo test -p strukt-app --all-targets --locked --offline
```

Expected: all commands exit 0 and every required focus/state/size contract is asserted.

- [ ] **Step 6: Commit accessibility and responsive behavior**

```bash
git add crates/strukt-ui crates/strukt-app
git commit -m "feat(ui): enforce accessible responsive composition"
```

### Task 14: Add deterministic M5.5 smoke and CI coverage

**Files:**
- Create: `crates/strukt-app/src/m5_5_smoke.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Create: `scripts/m5-5-visual-foundation-smoke.sh`
- Create: `scripts/check-ui-semantics.sh`
- Modify: `.github/workflows/ci.yml`
- Create: `docs/evidence/m5-5-visual-foundation.md`

- [ ] **Step 1: Write the smoke entrypoint contract**

Add a deterministic mode that constructs the app without opening a native window, registers all built-in contributions, resolves both themes, visits every activity, opens/closes every optional region, promotes/demotes the terminal, searches the command center, restores a valid and invalid snapshot, and prints exactly:

```text
M5.5 visual foundation smoke passed
```

It must not read user state, spawn a shell, contact a remote host, or modify a workspace.

- [ ] **Step 2: Add the shell wrapper**

The script follows existing smoke-script conventions, resolves the repository root, uses locked offline Cargo, and fails on any unexpected exit status or missing success line.

- [ ] **Step 3: Enforce semantic chrome ownership**

Add `scripts/check-ui-semantics.sh` to reject direct `iced::Color`, `Color::from_rgb`, and literal RGB/hex chrome declarations under `crates/strukt-app/src/view/`. Permit only documented editor syntax and terminal parser modules outside that directory. Run the check in CI and document any future exception in the script with its exact file and reason.

- [ ] **Step 4: Add CI execution on all supported runners**

Run the semantic check and M5.5 smoke in the existing macOS/Windows/Ubuntu matrix alongside the existing milestone smokes. Use Bash on macOS/Ubuntu and the repository's established PowerShell marker-check pattern on Windows.

- [ ] **Step 5: Run the new checks locally**

```bash
./scripts/check-ui-semantics.sh
./scripts/m5-5-visual-foundation-smoke.sh
```

Expected output ends with `M5.5 visual foundation smoke passed` and exits 0.

- [ ] **Step 6: Commit deterministic verification**

```bash
git add crates/strukt-app/src/m5_5_smoke.rs crates/strukt-app/src/main.rs scripts/check-ui-semantics.sh scripts/m5-5-visual-foundation-smoke.sh .github/workflows/ci.yml docs/evidence/m5-5-visual-foundation.md
git commit -m "test: add M5.5 visual foundation smoke"
```

### Task 15: Run the complete milestone gate and close documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`
- Modify: `docs/evidence/m5-5-visual-foundation.md`
- Modify: `docs/plans/0010-m5-5-visual-and-interaction-foundation.md`

- [ ] **Step 1: Run formatting, linting, unit, and build gates**

```bash
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
cargo build --workspace --all-targets --locked --offline
```

Expected: every command exits 0 with no warnings promoted by Clippy.

- [ ] **Step 2: Run every milestone smoke**

```bash
fixture="$(mktemp -d)"
printf 'strukt\n' > "$fixture/strukt-smoke.txt"
printf 'strukt\n' > "$fixture/strukt-editor-smoke.txt"
cargo run -p strukt-app --locked --offline -- --smoke-test
cargo run -p strukt-app --locked --offline -- --workspace-files-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --editor-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --terminal-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --language-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --m2-integration-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --session-smoke "$fixture"
cargo run -p strukt-app --locked --offline -- --remote-smoke "$fixture"
bash scripts/m5-remote-sessions-smoke.sh
bash scripts/m5-5-visual-foundation-smoke.sh
```

Expected: every command exits 0 and prints its documented success marker.

- [ ] **Step 3: Record performance evidence**

Build a release binary, run the deterministic native startup smoke ten times, and record the median wall-clock result next to the existing M1 startup evidence. Exercise theme switching and drawer promotion in the deterministic composition test while asserting that terminal, language, session, and remote runtime IDs do not change. Record the large-terminal, editor-input, explorer, diagnostics, session-history, and reconnect regression results from the retained suites. Any measurable startup regression above 10 percent or interaction stall above one 16.7 ms frame is a blocking finding unless the PR documents and explicitly accepts the release risk.

- [ ] **Step 4: Complete human visual walkthroughs**

On macOS, and on Windows before merge, verify dark and light themes at constrained, standard, and wide window sizes. Walk Files, Search, Git, editor, Problems, context, terminal drawer/split/full, local sessions, connections, remote workspace, persistent sessions, Settings, command center, keyboard-only navigation, error recovery, and restart restoration. Capture redacted screenshots in `docs/evidence/` only when they add review value.

- [ ] **Step 5: Run exact-head CI**

Push the branch, wait for GitHub Actions, and record the passing workflow URL and exact commit SHA in the evidence document. Any code change after that run invalidates the evidence and requires another exact-head run.

- [ ] **Step 6: Complete agentic review**

Run a review against the approved spec, this plan, the full diff, architecture boundaries, security-sensitive remote states, persistence redaction, accessibility, and regression evidence. Resolve every blocking finding and rerun affected gates.

- [ ] **Step 7: Update source-of-truth documentation**

Document the new shell, keyboard entrypoints, themes, and unchanged startup command in `README.md`. Mark M5.5 `Complete` in roadmap/tracker only after all acceptance criteria pass. Link the umbrella issue, all slice PRs, and the evidence document. Mark this plan `Complete` and check every executed step.

- [ ] **Step 8: Run forj merge-readiness checks**

```bash
forj check docs/specs/0010-m5-5-visual-and-interaction-foundation.md
forj check docs/plans/0010-m5-5-visual-and-interaction-foundation.md
git diff --check
git status --short
```

Expected: checks exit 0; only intended files appear before the final commit.

- [ ] **Step 9: Commit milestone closure**

```bash
git add README.md docs/roadmap.md docs/tracker.md docs/evidence/m5-5-visual-foundation.md docs/plans/0010-m5-5-visual-and-interaction-foundation.md
git commit -m "docs: complete M5.5 visual foundation"
```

## Acceptance traceability

| Approved requirement | Implemented by | Verified by |
| --- | --- | --- |
| Quiet Precision dark/light visual system | Tasks 2–3, 7 | Theme tests, component tests, walkthrough |
| Adaptive canvas and stable navigation | Tasks 4, 7 | Composition tests, app tests, M5.5 smoke |
| Promoteable bottom drawer | Tasks 4, 10 | Placement identity tests, terminal smoke |
| Unified command center | Tasks 6–7 | Command tests, keyboard walkthrough |
| Modular removable contributions | Tasks 4, 6 | Registration/unregistration tests |
| Future custom-theme boundary | Task 2 | Schema/validation/fallback tests |
| Workspace composition persistence | Task 5 | Corruption, clamp, and round-trip tests |
| Local M1–M3 workflow parity | Tasks 8–11 | Unit tests and M1–M3 smokes |
| Remote M4–M5 workflow parity | Task 12 | Unit tests, M4/M5 smokes, live walkthrough |
| Keyboard/accessibility/responsive behavior | Task 13 | Contract tests and walkthrough |
| Performance and runtime continuity | Tasks 10, 13, 15 | Identity assertions, retained load suites, recorded release measurements |
| macOS/Windows/Linux build confidence | Tasks 14–15 | CI matrix and exact-head evidence |
| No new product capabilities | All tasks | Spec review and final agentic review |

## Rollback and recovery

- Each slice must be independently revertible without changing persisted domain data.
- Unknown or invalid theme definitions resolve to the built-in Quiet Precision theme and emit a structured warning.
- Unknown surface or contribution IDs in saved shell state are removed during restore and replaced by a valid default canvas.
- A failed remote view migration falls back to the existing remote operation model; no remote command is retried automatically.
- If a slice cannot preserve its feature smoke, do not merge it and do not start the next slice.

## Completion definition

M5.5 is complete only when every checkbox is checked, all five delivery slices are merged, exact-head CI passes, macOS and Windows walkthrough evidence is linked, agentic review has no blocking findings, every M1–M5 smoke remains green, and roadmap/tracker status is `Complete`. Passing the new visual smoke alone is not completion.
