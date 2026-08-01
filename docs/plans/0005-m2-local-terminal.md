# M2.3 Local Terminal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the representative drawer with bounded, GPU-rendered local PTY/ConPTY terminal tabs and splits that restore only stopped presentation state.

**Architecture:** Add an Iced-independent `strukt-terminal` crate for the grid, VTE reduction, selection, links, layout, process state, transport contract, and fair runtime scheduler. Use `portable-pty` only behind the transport adapter, `vte` only behind normalized parser actions, existing opaque workspace contributions for persistence, and a focused Iced custom widget that consumes immutable snapshots.

**Tech Stack:** Rust 1.97.1, `portable-pty` 0.9, `vte` 0.15, `unicode-width`, bounded `std::sync::mpsc` channels, Serde, Tokio task wiring, Iced 0.14 advanced widget/WGPU renderer, GitHub Actions macOS/Ubuntu/Windows matrix.

---

## File map

- `crates/strukt-terminal/src/id.rs`: opaque terminal tab and pane identifiers.
- `crates/strukt-terminal/src/cell.rs`: cell content, width, color, and attributes.
- `crates/strukt-terminal/src/grid.rs`: visible grid, alternate screen, cursor, modes, scrollback, and immutable snapshots.
- `crates/strukt-terminal/src/parser.rs`: `vte::Perform` adapter and bounded escape/OSC reduction.
- `crates/strukt-terminal/src/selection.rs`: terminal coordinates, selection extraction, hyperlink and URL detection, paste framing.
- `crates/strukt-terminal/src/layout.rs`: tabs, recursive split tree, focus, collapse, rename, and stopped restoration.
- `crates/strukt-terminal/src/transport.rs`: shared spawn/input/output/resize/exit/terminate types and trait.
- `crates/strukt-terminal/src/portable.rs`: `portable-pty` Unix PTY/Windows ConPTY implementation.
- `crates/strukt-terminal/src/runtime.rs`: pane workers, bounded queues, fair draining, process transitions, and snapshots.
- `crates/strukt-terminal/src/bin/terminal-fixture.rs`: deterministic native contract/smoke child.
- `crates/strukt-persistence/src/terminal_store.rs`: versioned terminal workspace contribution.
- `crates/strukt-app/src/terminal.rs`: application-facing terminal surfaces and Iced event conversion.
- `crates/strukt-app/src/terminal_widget.rs`: custom GPU terminal widget.
- `crates/strukt-app/src/app.rs`: terminal commands, background events, persistence, and smoke orchestration.
- `crates/strukt-app/src/view.rs`: drawer/canvas chrome and controls.

## Task 1: Scaffold the terminal domain and cell model

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/strukt-terminal/Cargo.toml`
- Create: `crates/strukt-terminal/src/lib.rs`
- Create: `crates/strukt-terminal/src/id.rs`
- Create: `crates/strukt-terminal/src/cell.rs`
- Create: `crates/strukt-terminal/tests/cells.rs`

- [x] **Step 1: Write the failing cell and identifier tests**

```rust
use strukt_terminal::{Cell, CellAttributes, CellWidth, Color, TerminalPaneId};

#[test]
fn pane_ids_are_unique_and_cells_reset_to_semantic_defaults() {
    assert_ne!(TerminalPaneId::new(), TerminalPaneId::new());
    let mut cell = Cell::default();
    cell.set_text("界", CellWidth::Wide).unwrap();
    cell.attributes = CellAttributes { bold: true, ..CellAttributes::default() };
    cell.foreground = Color::Indexed(42);
    cell.reset();
    assert_eq!(cell, Cell::default());
}

#[test]
fn cells_bound_combining_text_and_reject_continuation_content() {
    let mut cell = Cell::default();
    cell.set_text("e\u{301}", CellWidth::Single).unwrap();
    assert_eq!(cell.text(), "e\u{301}");
    assert!(cell.set_text("x", CellWidth::Continuation).is_err());
}
```

- [x] **Step 2: Run the test and verify the crate is absent**

Run: `cargo test -p strukt-terminal --test cells --locked --offline`

Expected: fail because `strukt-terminal` is not a workspace member.

- [x] **Step 3: Add the crate and minimal public types**

Add workspace dependencies `portable-pty = "0.9.0"`, `vte = "0.15.0"`, and
`unicode-width = "0.2"`. Define ID newtypes around a process-local atomic counter:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct TerminalPaneId(u64);

impl TerminalPaneId {
    #[must_use]
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}
```

Define `Color::{Default, Indexed(u8), Rgb(u8,u8,u8)}`, `CellWidth`, bounded cell
text, and explicit attributes. A continuation cell must always have empty text.

- [x] **Step 4: Run focused tests and strict lint**

Run: `cargo test -p strukt-terminal --test cells --locked --offline`

Expected: 2 passed.

Run: `cargo clippy -p strukt-terminal --all-targets --locked --offline -- -D warnings`

Expected: pass.

- [x] **Step 5: Commit the cell foundation**

```bash
git add Cargo.toml Cargo.lock crates/strukt-terminal
git commit -m "feat: add terminal cell foundation"
```

## Task 2: Build the bounded grid, cursor, screen modes, and snapshots

**Files:**

- Create: `crates/strukt-terminal/src/grid.rs`
- Modify: `crates/strukt-terminal/src/lib.rs`
- Create: `crates/strukt-terminal/tests/grid.rs`

- [x] **Step 1: Write failing grid invariants**

```rust
use strukt_terminal::{Grid, GridSize, TerminalSnapshot};

#[test]
fn wide_cells_wrap_without_orphan_continuations() {
    let mut grid = Grid::new(GridSize::new(2, 4).unwrap(), 10);
    grid.print("abc界");
    let snapshot = grid.snapshot();
    assert_eq!(snapshot.plain_text(), "abc \n界  ");
    assert!(snapshot.rows().iter().flatten().all(|cell| cell.is_structurally_valid()));
}

#[test]
fn scrollback_is_bounded_and_alternate_screen_does_not_pollute_it() {
    let mut grid = Grid::new(GridSize::new(2, 3).unwrap(), 2);
    grid.print("1\r\n2\r\n3\r\n4");
    assert_eq!(grid.scrollback_len(), 2);
    grid.enter_alternate_screen();
    grid.print("alt");
    grid.leave_alternate_screen();
    assert_eq!(grid.scrollback_len(), 2);
}
```

- [x] **Step 2: Run and observe missing grid types**

Run: `cargo test -p strukt-terminal --test grid --locked --offline`

Expected: compile failure for `Grid`.

- [x] **Step 3: Implement grid state and immutable snapshots**

Implement checked nonzero `GridSize`, primary and alternate buffers, cursor and
saved cursor, scroll margins, wrap-pending state, terminal modes, and a `VecDeque`
scrollback capped at construction. `TerminalSnapshot` owns only the selected
viewport, cursor, title, modes, revision, and notices.

```rust
pub struct Grid {
    size: GridSize,
    primary: Screen,
    alternate: Screen,
    active: ActiveScreen,
    scrollback: VecDeque<Row>,
    scrollback_limit: usize,
    revision: u64,
}

impl Grid {
    pub fn resize(&mut self, size: GridSize) -> ResizeOutcome;
    pub fn snapshot(&self, viewport_offset: usize) -> TerminalSnapshot;
    pub fn erase_in_display(&mut self, mode: EraseDisplay);
    pub fn scroll_up(&mut self, lines: usize);
}
```

- [x] **Step 4: Add resize, reflow, cursor, erasure, and alternate-screen cases**

Cover shrinking through a wide glyph, scroll-region isolation, cursor clamping,
insert/delete line, insert/delete characters, reverse index, and snapshot viewport
bounds with explicit assertions in `tests/grid.rs`.

- [x] **Step 5: Run focused and crate tests**

Run: `cargo test -p strukt-terminal --locked --offline`

Expected: all terminal tests pass.

- [x] **Step 6: Commit the grid model**

```bash
git add crates/strukt-terminal/src crates/strukt-terminal/tests/grid.rs
git commit -m "feat: model bounded terminal grids"
```

## Task 3: Normalize VTE parser actions and terminal modes

**Files:**

- Create: `crates/strukt-terminal/src/parser.rs`
- Modify: `crates/strukt-terminal/src/grid.rs`
- Modify: `crates/strukt-terminal/src/lib.rs`
- Create: `crates/strukt-terminal/tests/parser.rs`

- [x] **Step 1: Write failing parser behavior tests**

```rust
use strukt_terminal::{Color, GridSize, TerminalModel};

#[test]
fn parser_applies_unicode_sgr_cursor_and_alternate_screen() {
    let mut terminal = TerminalModel::new(GridSize::new(3, 12).unwrap(), 100);
    terminal.advance(b"plain \x1b[1;38;2;1;2;3mred\x1b[0m");
    let snapshot = terminal.snapshot(0);
    assert_eq!(snapshot.cell(0, 6).unwrap().foreground, Color::Rgb(1, 2, 3));
    assert!(snapshot.cell(0, 6).unwrap().attributes.bold);

    terminal.advance(b"\x1b[?1049halt\x1b[?1049l");
    assert!(terminal.snapshot(0).plain_text().contains("plain"));
}

#[test]
fn oversized_osc_is_discarded_and_counted_without_growth() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 8).unwrap(), 10);
    let oversized = format!("\x1b]2;{}\x07", "x".repeat(9_000));
    terminal.advance(oversized.as_bytes());
    assert_eq!(terminal.snapshot(0).title(), None);
    assert_eq!(terminal.diagnostics().discarded_sequences, 1);
}
```

- [x] **Step 2: Run and verify missing parser/model APIs**

Run: `cargo test -p strukt-terminal --test parser --locked --offline`

Expected: compile failure.

- [x] **Step 3: Implement `vte::Perform` with bounded intermediate state**

`TerminalModel` owns `vte::Parser`, `Grid`, an 8 KiB OSC accumulator, hyperlink
state, and counters. Implement printable characters, execute controls, CSI cursor,
erase, insert/delete, scrolling, SGR, DECSET/DECRST modes, OSC title, and OSC 8.

```rust
pub struct TerminalModel {
    parser: vte::Parser,
    performer: TerminalPerformer,
}

impl TerminalModel {
    pub fn advance(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.parser.advance(&mut self.performer, *byte);
        }
    }
}
```

Keep unsupported CSI/OSC actions no-op and increment a saturating diagnostic count.

- [x] **Step 4: Add conformance fixtures**

Add table-driven fixtures for 16/256/true color, underline/italic/inverse, cursor
save/restore, margins, bracketed paste, application cursor keys, focus reporting,
mouse mode flags, OSC title, OSC 8, malformed UTF-8, and split escape chunks.

- [x] **Step 5: Run parser and complete crate tests**

Run: `cargo test -p strukt-terminal --locked --offline`

Expected: pass.

- [x] **Step 6: Commit VTE reduction**

```bash
git add crates/strukt-terminal
git commit -m "feat: parse bounded terminal streams"
```

## Task 4: Add selection, links, input encoding, and paste policy

**Files:**

- Create: `crates/strukt-terminal/src/selection.rs`
- Modify: `crates/strukt-terminal/src/grid.rs`
- Modify: `crates/strukt-terminal/src/lib.rs`
- Create: `crates/strukt-terminal/tests/interaction.rs`

- [x] **Step 1: Write failing interaction tests**

```rust
use strukt_terminal::{GridSize, PasteDecision, Selection, TerminalModel};

#[test]
fn selection_extracts_wrapped_wide_text_and_links_are_explicit() {
    let mut terminal = TerminalModel::new(GridSize::new(3, 20).unwrap(), 100);
    terminal.advance("go https://example.com/界".as_bytes());
    let selection = Selection::linear((0, 0), (1, 3));
    assert!(terminal.copy_text(&selection).unwrap().contains("界"));
    let link = terminal.links().next().unwrap();
    assert_eq!(link.target(), "https://example.com/界");
    assert!(!link.opened());
}

#[test]
fn paste_removes_nul_frames_bracketed_mode_and_requires_large_confirmation() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 10).unwrap(), 10);
    terminal.advance(b"\x1b[?2004h");
    assert_eq!(terminal.prepare_paste("a\0b", false), PasteDecision::Send(b"\x1b[200~ab\x1b[201~".to_vec()));
    assert!(matches!(terminal.prepare_paste(&"x".repeat(1_048_577), false), PasteDecision::Confirm { .. }));
}
```

- [x] **Step 2: Run and confirm missing interaction types**

Run: `cargo test -p strukt-terminal --test interaction --locked --offline`

Expected: compile failure.

- [x] **Step 3: Implement coordinate-safe selection and input policy**

Implement viewport-to-buffer coordinate conversion, linear selection normalization,
wide-cell snapping, copy extraction, OSC 8 ranges, bounded URL regex detection for
`http`, `https`, `mailto`, and `file`, application cursor-key encoding, focus bytes,
mouse-report bytes, NUL removal, bracketed paste, and 1 MiB confirmation.

- [x] **Step 4: Run the focused interaction suite**

Run: `cargo test -p strukt-terminal --test interaction --locked --offline`

Expected: all cases pass.

- [x] **Step 5: Commit terminal interaction policy**

```bash
git add crates/strukt-terminal
git commit -m "feat: add terminal selection and input policy"
```

## Task 5: Model tabs, recursive splits, and stopped restoration

**Files:**

- Create: `crates/strukt-terminal/src/layout.rs`
- Modify: `crates/strukt-terminal/src/lib.rs`
- Create: `crates/strukt-terminal/tests/layout.rs`

- [x] **Step 1: Write failing layout reducer tests**

```rust
use strukt_terminal::{PaneState, SplitAxis, TerminalWorkspace};

#[test]
fn split_close_and_focus_preserve_a_valid_tree() {
    let mut workspace = TerminalWorkspace::default();
    let first = workspace.create_tab("Terminal 1", "/workspace").unwrap();
    let second = workspace.split_focused(SplitAxis::Vertical).unwrap();
    assert_eq!(workspace.focused_pane(), Some(second));
    workspace.close_pane(second).unwrap();
    assert_eq!(workspace.focused_pane(), Some(first));
    assert!(workspace.active_tab().unwrap().root().is_pane());
}

#[test]
fn restored_panes_are_stopped_and_never_retain_commands() {
    let snapshot = fixture_snapshot_with_nested_split();
    let workspace = TerminalWorkspace::restore(snapshot).unwrap();
    assert!(workspace.panes().all(|pane| matches!(pane.state(), PaneState::Stopped)));
    assert!(workspace.panes().all(|pane| pane.command().is_none()));
}
```

- [x] **Step 2: Run and verify reducer types are absent**

Run: `cargo test -p strukt-terminal --test layout --locked --offline`

Expected: compile failure.

- [x] **Step 3: Implement the layout tree and lifecycle transitions**

Define `LayoutNode::{Pane(TerminalPaneId), Split { axis, ratio_basis_points,
first, second }}`, ordered tabs, active/focused IDs, and `PaneState::{Stopped,
Starting, Running, Exited, Failed, Backpressured}`. Enforce ratios 1000..=9000,
unique IDs, nonempty tabs, deterministic collapse, and no persisted command field.

- [x] **Step 4: Add invalid snapshot and independent-tab cases**

Test duplicate IDs, missing focused IDs, ratios outside bounds, empty split branches,
tab activation, rename validation, close confirmation, restart transition, and two
tabs whose pane state changes remain isolated.

- [x] **Step 5: Run layout and crate tests**

Run: `cargo test -p strukt-terminal --locked --offline`

Expected: pass.

- [x] **Step 6: Commit terminal layout**

```bash
git add crates/strukt-terminal
git commit -m "feat: model terminal tabs and splits"
```

## Task 6: Persist only terminal presentation state

**Files:**

- Create: `crates/strukt-persistence/src/terminal_store.rs`
- Modify: `crates/strukt-persistence/src/lib.rs`
- Modify: `crates/strukt-persistence/Cargo.toml`
- Modify: `crates/strukt-terminal/src/layout.rs`
- Create: `crates/strukt-persistence/tests/terminal_store.rs`

- [x] **Step 1: Write failing schema round-trip and privacy tests**

```rust
use serde_json::Value;
use strukt_persistence::TerminalSessionSnapshot;

#[test]
fn terminal_snapshot_round_trips_layout_without_runtime_content() {
    let snapshot = nested_terminal_snapshot();
    let value = serde_json::to_value(&snapshot).unwrap();
    let object = value.to_string();
    for forbidden in ["scrollback", "environment", "command", "child_id", "output"] {
        assert!(!object.contains(forbidden));
    }
    assert_eq!(serde_json::from_value::<TerminalSessionSnapshot>(value).unwrap(), snapshot);
}

#[test]
fn unknown_fields_survive_the_workspace_contribution_round_trip() {
    let value: Value = serde_json::json!({"schema_version":1,"tabs":[],"future":{"kept":true}});
    assert_eq!(round_trip_terminal_contribution(value.clone()), value);
}
```

- [x] **Step 2: Run and confirm the schema is missing**

Run: `cargo test -p strukt-persistence --test terminal_store --locked --offline`

Expected: compile failure.

- [x] **Step 3: Implement versioned snapshot DTOs and conversion**

Use explicit schema version 1 DTOs for tab, recursive node, pane, axis, ratio, active
tab, and focused pane. `TerminalWorkspace::snapshot()` emits them and
`TerminalWorkspace::restore()` validates them, converts every pane to stopped, and
discards no opaque workspace contribution data.

- [x] **Step 4: Add corruption and migration cases**

Test unsupported versions, duplicate IDs, invalid focus, invalid ratios, missing
working directories, empty tabs, and last-valid workspace fallback through
`WorkspaceStore`.

- [x] **Step 5: Run persistence and workspace suites**

Run: `cargo test -p strukt-persistence -p strukt-workspace --locked --offline`

Expected: pass.

- [x] **Step 6: Commit the terminal schema**

```bash
git add crates/strukt-persistence crates/strukt-terminal
git commit -m "feat: persist stopped terminal layouts"
```

## Task 7: Implement the shared PTY/ConPTY transport contract

**Files:**

- Create: `crates/strukt-terminal/src/transport.rs`
- Create: `crates/strukt-terminal/src/portable.rs`
- Create: `crates/strukt-terminal/src/bin/terminal-fixture.rs`
- Modify: `crates/strukt-terminal/src/lib.rs`
- Modify: `crates/strukt-terminal/Cargo.toml`
- Create: `crates/strukt-terminal/tests/transport_contract.rs`

- [x] **Step 1: Write the native transport contract tests**

```rust
#[test]
fn native_transport_spawns_writes_resizes_exits_and_isolates() {
    let fixture = env!("CARGO_BIN_EXE_terminal-fixture");
    let transport = PortableTransport::new();
    let mut first = transport.spawn(request(fixture, "echo", 24, 80)).unwrap();
    let mut second = transport.spawn(request(fixture, "echo", 10, 40)).unwrap();
    first.write("héllo\n".as_bytes()).unwrap();
    second.write(b"other\n").unwrap();
    first.resize(TerminalSize::new(30, 100).unwrap()).unwrap();
    assert!(read_until(&first, "héllo", Duration::from_secs(5)));
    assert!(read_until(&second, "other", Duration::from_secs(5)));
    assert_eq!(first.wait(Duration::from_secs(5)).unwrap().code(), Some(0));
    assert_eq!(second.wait(Duration::from_secs(5)).unwrap().code(), Some(0));
}

#[test]
fn native_transport_terminates_a_long_running_fixture() {
    let mut child = PortableTransport::new().spawn(request(fixture(), "wait", 24, 80)).unwrap();
    child.terminate(Duration::from_millis(500)).unwrap();
    assert!(child.wait(Duration::from_secs(2)).unwrap().was_terminated());
}
```

- [x] **Step 2: Run and verify transport types are absent**

Run: `cargo test -p strukt-terminal --test transport_contract --locked --offline`

Expected: compile failure.

- [x] **Step 3: Define the object-safe transport contract**

```rust
pub trait TerminalTransport: Send + Sync {
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn TerminalProcess>, TransportError>;
}

pub trait TerminalProcess: Send {
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError>;
    fn resize(&mut self, size: TerminalSize) -> Result<(), TransportError>;
    fn try_read(&mut self) -> Result<Option<OutputChunk>, TransportError>;
    fn try_wait(&mut self) -> Result<Option<ExitStatus>, TransportError>;
    fn terminate(&mut self, grace: Duration) -> Result<(), TransportError>;
}
```

Use sequence-tagged output chunks capped at 64 KiB. Validate executable, absolute
working directory, rows/columns, and environment additions before calling the
adapter.

- [x] **Step 4: Implement `portable-pty` behind dedicated reader/writer workers**

Open the native PTY with the requested size, spawn on the slave, close the slave in
the parent, and put the blocking reader on its own thread. Use a `sync_channel(1024)`
plus a shared 4 MiB byte budget. Keep writer, master resize handle, child wait, and
termination ownership together so dropping a process cannot detach a child.

- [x] **Step 5: Run the contract natively and cross-compile it**

Run: `cargo test -p strukt-terminal --test transport_contract --locked --offline`

Expected: pass on macOS.

Run: `cargo check -p strukt-terminal --target x86_64-unknown-linux-gnu --locked --offline`

Expected: pass.

Run: `cargo check -p strukt-terminal --target x86_64-pc-windows-msvc --locked --offline`

Expected: pass; this proves Windows type and adapter portability. The hosted
Windows CI contract test in Task 10 must execute ConPTY before merge readiness.

- [x] **Step 6: Commit native transport**

```bash
git add Cargo.toml Cargo.lock crates/strukt-terminal
git commit -m "feat: run local PTY and ConPTY processes"
```

## Task 8: Add bounded runtime scheduling and lifecycle reduction

**Files:**

- Create: `crates/strukt-terminal/src/runtime.rs`
- Modify: `crates/strukt-terminal/src/lib.rs`
- Create: `crates/strukt-terminal/tests/runtime.rs`

- [x] **Step 1: Write failing fair-drain and stale-event tests**

```rust
#[test]
fn ready_panes_drain_round_robin_with_per_pane_and_aggregate_budgets() {
    let mut runtime = fixture_runtime([(pane(1), 2_000_000), (pane(2), 64)]);
    let batch = runtime.drain(DrainBudget::new(256 * 1024, 1024 * 1024));
    assert!(batch.bytes_for(pane(1)) <= 256 * 1024);
    assert_eq!(batch.bytes_for(pane(2)), 64);
    assert!(batch.changed_panes().contains(&pane(2)));
}

#[test]
fn stale_output_and_exit_events_cannot_cross_a_restart_generation() {
    let mut runtime = fixture_runtime([(pane(1), 0)]);
    let old = runtime.generation(pane(1)).unwrap();
    runtime.restart(pane(1), spawn_request()).unwrap();
    runtime.apply_output(pane(1), old, chunk(b"stale")).unwrap();
    assert!(!runtime.snapshot(pane(1)).unwrap().plain_text().contains("stale"));
}
```

- [x] **Step 2: Run and confirm runtime types are missing**

Run: `cargo test -p strukt-terminal --test runtime --locked --offline`

Expected: compile failure.

- [x] **Step 3: Implement generation-scoped pane runtimes**

Track process, generation, sequence, model, viewport, pending input, queue state,
last progress, and pane-local error. Spawn/restart increments generation. Drain
ready panes round-robin, rejects old generation/sequence events, parses within both
budgets, updates backpressure at 250 ms, and updates sustained-output state at two
seconds.

- [x] **Step 4: Add spawn failure, resize failure, exit, termination, and noisy-pane cases**

Use a fake transport to deterministically assert every failure stays pane-local,
another pane advances, and snapshots are revision-based.

- [x] **Step 5: Run terminal tests and strict lint**

Run: `cargo test -p strukt-terminal --locked --offline`

Run: `cargo clippy -p strukt-terminal --all-targets --locked --offline -- -D warnings`

Expected: both pass.

- [x] **Step 6: Commit the runtime**

```bash
git add crates/strukt-terminal
git commit -m "feat: schedule bounded terminal runtimes"
```

## Task 9: Integrate terminal state, commands, persistence, and native UI

**Files:**

- Modify: `crates/strukt-app/Cargo.toml`
- Create: `crates/strukt-app/src/terminal.rs`
- Create: `crates/strukt-app/src/terminal_widget.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-theme/src/tokens.rs`
- Modify: `crates/strukt-theme/tests/builtin_themes.rs`

- [x] **Step 1: Write failing application reducer tests**

Add tests in `crates/strukt-app/src/main.rs` proving:

```rust
#[test]
fn terminal_commands_require_a_workspace_and_never_spawn_on_open() {
    let mut app = StruktApp::default();
    assert_eq!(app.update(Message::NewTerminal).units(), 0);
    app.workspace = Some(workspace_state(tempdir().unwrap().path()));
    assert!(app.terminal.workspace().tabs().is_empty());
    assert_eq!(app.update(Message::NewTerminal).units(), 1);
}

#[test]
fn restored_terminal_contribution_creates_only_stopped_placeholders() {
    let app = restore_app_with_terminal_contribution(nested_terminal_snapshot());
    assert!(app.terminal.workspace().panes().all(|pane| pane.state().is_stopped()));
    assert_eq!(app.terminal.running_processes(), 0);
}
```

Also test split, focus, rename, close confirmation, restart, paste confirmation,
stale runtime batches, persistence coalescing, workspace replacement, and capability
disablement.

- [x] **Step 2: Run focused app tests and observe missing messages/surfaces**

Run: `cargo test -p strukt-app terminal --locked --offline`

Expected: compile failure.

- [x] **Step 3: Wire terminal commands and background polling**

Add `TerminalSurfaces` wrapping `TerminalWorkspace` and `TerminalRuntime`. Add typed
messages for create/split/focus/input/resize/scroll/selection/copy/paste/link/
restart/close plus `PollTerminal` and generation-scoped completions. Poll only while
a process runs or output remains. Persist presentation changes through the existing
coalesced workspace snapshot path.

- [x] **Step 4: Replace the representative drawer and add custom widget snapshots**

Render tab chrome, recursive split containers, pane state, local boundary, title,
working directory, errors, exit code, backpressure, restart, and close. The custom
widget receives `&TerminalSnapshot` plus focused/selection state and publishes only
`TerminalWidgetEvent` values.

```rust
pub enum TerminalWidgetEvent {
    Focus(TerminalPaneId),
    Input(TerminalPaneId, Vec<u8>),
    Resize(TerminalPaneId, TerminalSize),
    Select(TerminalPaneId, SelectionAction),
    Scroll(TerminalPaneId, i32),
    ActivateLink(TerminalPaneId, LinkId),
}
```

Use renderer text primitives with clipped cell rectangles, semantic ANSI palette,
selection background, and block/bar/underline cursor geometry. Map native composed
text and terminal modes in `terminal.rs`; keep widget drawing free of domain mutation.

- [x] **Step 5: Add theme and keyboard coverage**

Add explicit ANSI 0-15 palette, terminal foreground, selection, cursor, link,
exited, and backpressure tokens to both built-in themes. Test platform command
shortcuts, composed text, application cursor keys, split/tab focus, copy, paste,
close, and drawer-to-canvas expansion.

- [x] **Step 6: Run app, theme, and full workspace tests**

Run: `cargo test -p strukt-app -p strukt-theme --locked --offline`

Run: `cargo test --workspace --all-targets --locked --offline`

Expected: pass.

- [x] **Step 7: Commit application integration**

```bash
git add Cargo.toml Cargo.lock crates/strukt-app crates/strukt-theme
git commit -m "feat: operate local terminal panes"
```

## Task 10: Add deterministic native smoke, stress, and CI gates

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-terminal/src/bin/terminal-fixture.rs`
- Create: `docs/evidence/m2-local-terminal-validation.md`

- [x] **Step 1: Write failing launch-mode and smoke contract tests**

```rust
#[test]
fn terminal_smoke_requires_the_exact_flag_and_one_existing_root() {
    assert_eq!(LaunchMode::from_args(["--terminal-smoke".into(), "fixture".into()]), LaunchMode::TerminalSmoke { root: "fixture".into() });
    for args in [vec!["--terminal-smoke".into()], vec!["--terminal-smokes".into(), "fixture".into()], vec!["--terminal-smoke".into(), "fixture".into(), "extra".into()]] {
        assert_eq!(LaunchMode::from_args(args), LaunchMode::Interactive);
    }
}
```

The end-to-end test must assert the exact marker:

```text
strukt terminal smoke: pty, unicode, ansi, resize, isolation, bounds, and restore passed
```

- [x] **Step 2: Implement `--terminal-smoke` without the user's shell**

Open the fixture workspace, spawn two instances of `terminal-fixture` through
`PortableTransport`, verify Unicode echo, ANSI attributes, resize acknowledgement,
isolated exit states, a nested split, 64 MiB bounded producer progress, quiet-pane
progress, snapshot persistence/restoration as stopped, and absence of `.strukt`.
Give the smoke a 30-second internal deadline and terminate every child on all paths.

- [x] **Step 3: Add all-platform workflow steps**

On macOS/Ubuntu use `mktemp -d`; on Windows use a GUID-named directory. Run the
built `strukt-app` executable with `--terminal-smoke`, require exit zero, match the
exact marker, and reject `.strukt`. Keep the Windows native startup smoke.

- [x] **Step 4: Run local smoke and complete local gate**

```bash
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
cargo build -p strukt-app --locked --offline
cargo run -p strukt-app --locked --offline -- --terminal-smoke <fixture>
cargo check -p strukt-app --target x86_64-unknown-linux-gnu --locked --offline
cargo clippy -p strukt-terminal -p strukt-persistence -p strukt-workspace \
  --target x86_64-pc-windows-msvc --all-targets --locked --offline -- -D warnings
```

Expected: every command passes; only the documented transitive `block 0.1.6`
future-compatibility warning may remain.

- [ ] **Step 5: Commit smoke and draft evidence**

```bash
git add .github/workflows/ci.yml crates/strukt-app crates/strukt-terminal \
  docs/evidence/m2-local-terminal-validation.md
git commit -m "test: validate local terminal workflows"
```

## Task 11: Complete review, native walkthrough, and delivery artifacts

**Files:**

- Modify: `README.md`
- Modify: `docs/evidence/m2-local-terminal-validation.md`
- Modify: `docs/plans/0005-m2-local-terminal.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`
- Modify: `docs/decisions/0001-native-ui-framework.md`

- [ ] **Step 1: Complete the manual macOS walkthrough**

Use a native app bundle and isolated workspace. Exercise default shell spawn,
Unicode/IME, ANSI programs, cursor, selection, copy/paste and large-paste consent,
links with explicit open, resize, tabs, nested splits, focus, rename, exit, restart,
close confirmation, light/dark themes, output load, stopped restoration, keyboard
traversal, accessibility exposure, and no `.strukt` metadata.

- [ ] **Step 2: Run full-slice review**

Review against the spec for PTY ownership and cleanup, ConPTY portability, process
tree termination, queue byte accounting, deadlocks, fairness, stale generations,
escape/OSC bounds, Unicode/wide-cell invariants, selection/link security, paste
framing and consent, persistence privacy, accidental restart, custom-widget event
routing, accessibility, rendering bounds, capability disablement, and M3/M4 scope
drift. Resolve all critical and important findings with focused regression tests.

- [ ] **Step 3: Record hosted and stress evidence**

Push the final implementation SHA and require macOS 14, Ubuntu 24.04, and Windows
Server 2022 jobs. Record run/job links, native ConPTY contract result, 64 MiB stress
metrics, exact smoke marker, manual results, review findings, and honest Iced/human
Windows limitations in `docs/evidence/m2-local-terminal-validation.md`.

- [ ] **Step 4: Update milestone artifacts**

Mark M2.3 complete while M2 remains in progress. Add spec, plan, issue, PR, and
evidence links to tracker and roadmap. Update README runtime instructions and ADR
0001 with the M2 terminal revalidation result. Keep language intelligence and final
M2 integration listed as remaining.

- [ ] **Step 5: Commit completion evidence**

```bash
git add README.md docs/evidence/m2-local-terminal-validation.md \
  docs/plans/0005-m2-local-terminal.md docs/roadmap.md docs/tracker.md \
  docs/decisions/0001-native-ui-framework.md
git commit -m "docs: record M2 local terminal validation"
```

- [ ] **Step 6: Require the exact evidence commit to pass before merge readiness**

Push the evidence-only commit, require the final macOS/Ubuntu/Windows matrix, update
the PR body with verification and substantive review findings, mark the PR ready,
and merge only under `docs/process/merge-policy.md`.

## Final verification

M2.3 is not complete until every acceptance criterion in
`docs/specs/0005-m2-local-terminal.md` has direct evidence, all Task 11 findings are
resolved or explicitly accepted, the issue and PR link the spec/plan/evidence, and
the exact final PR head is green on macOS, Ubuntu, and Windows. M2 remains in
progress after this merge.
