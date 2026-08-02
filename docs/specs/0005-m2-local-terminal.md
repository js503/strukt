# M2.3 Local Terminal

- Status: Approved
- Date: 2026-07-31
- Parent spec:
  [`0003-local-development-workspace.md`](0003-local-development-workspace.md)
- Product foundation:
  [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)
- Spatial reference:
  [`../mockups/workspace-shell/focus-context.html`](../mockups/workspace-shell/focus-context.html)

## Summary

M2.3 replaces the representative terminal drawer with real, local, ephemeral
terminal tabs and split panes. Every running pane owns one operating-system pseudo
terminal for the lifetime of the `strukt` application. Unix uses a PTY and Windows
uses ConPTY through one shared transport contract.

The terminal engine is a first-class feature module rather than application glue.
It owns terminal identifiers, ANSI emulation, a bounded grid and scrollback,
selection, detected links, tab and split layout, process state, and immutable
renderer snapshots. Iced owns native event routing and GPU-backed presentation but
does not own terminal process or emulation state.

M2.3 deliberately does not make processes durable. Application restart restores
names, layout, working-directory hints, and stopped placeholders; it never restarts
commands. M3 will add durable local process ownership behind the transport boundary
established here, while M5 will reuse the same terminal model for remote sessions.

## Goals

- Spawn the user's default local shell in the open workspace without executing a
  workspace-provided command.
- Support multiple independently named terminal tabs and recursively split panes.
- Provide Unicode input, ANSI styling, cursor state, selection, clipboard actions,
  explicit link opening, resize propagation, exit status, restart, and close.
- Keep output, parsing, rendering snapshots, and persistence bounded.
- Prevent one noisy pane from starving another pane, the editor, or file navigation.
- Restore stopped terminal placeholders and layout without restoring output,
  environment contents, handles, or running commands.
- Provide one cross-platform PTY/ConPTY contract that M3 and M5 can replace without
  changing the terminal model or application-facing commands.
- Revalidate Iced for a real custom terminal surface on macOS and hosted Windows.

## Non-goals

- Processes that survive application exit, detach/reattach, named persistent
  sessions, or background daemons.
- SSH, remote shells, remote files, remote helpers, or tmux integration.
- Shell-command history synchronization or session replay.
- Images, sixel, kitty graphics, inline media, ligatures, or arbitrary font shaping.
- Full DEC hardware-terminal compatibility, serial terminals, or terminal-server
  protocols.
- Automatically installing shells, changing shell configuration, or running shell
  integration scripts.
- Automatically opening detected links.
- Persisting output, input, environment variables, credentials, process handles, or
  arbitrary commands.
- Human Windows packaging and complete visual certification, which remains an M9
  gate.

## Decisions

### Owned terminal model over embedded terminal applications

`strukt-terminal` owns a focused terminal grid and layout model. A small streaming
VTE parser provides escape-sequence recognition; normalized parser actions mutate
the owned model. This keeps product identifiers, bounds, persistence, and future
session-provider contracts under `strukt` control.

Embedding the Alacritty or WezTerm application model is rejected for this slice.
Those implementations are excellent references but bring configuration, event, or
multiplexer ownership that conflicts with the M2/M3 boundary.

### Cross-platform transport adapter

A transport contract exposes:

- spawn with shell program, arguments, working directory, environment additions,
  and initial rows and columns;
- ordered input bytes;
- bounded output chunks with a monotonically increasing sequence;
- resize;
- non-blocking exit observation;
- graceful termination followed by bounded forced termination;
- child identity only for diagnostics, never persistence.

The default implementation uses a maintained cross-platform PTY library that maps
to Unix PTYs and Windows ConPTY. Blocking reader and writer operations run on
dedicated workers. No blocking process call executes on Iced's update thread.

Transport output remains bytes until parsed. Invalid UTF-8 cannot crash or poison
the model; the terminal parser applies the replacement behavior required by the
terminal stream rather than treating output as a source document.

### Ephemeral process lifecycle

The default shell is resolved from the current user's platform environment:

- macOS and Linux use the configured login shell when it is an executable absolute
  path, otherwise the platform's standard shell fallback;
- Windows prefers the user's configured PowerShell profile when available and
  otherwise uses the platform command shell fallback;
- an explicit user terminal profile may replace the program and arguments;
- workspace-owned profile commands require the workspace trust flow and are outside
  this slice.

New panes start in the canonical workspace root. A sibling working-directory hint
may be reused only after a trusted transport event reports an existing accessible
directory. M2.3 does not inject shell integration, so this hint is normally the
initial root.

Closing a running pane requires an explicit confirmation unless it is the result of
application shutdown. Restart terminates the current child, preserves the pane ID,
name, layout position, and working-directory hint, clears in-memory output, then
spawns a new default shell. An exited pane retains its exit status and current
scrollback until closed or restarted.

Application shutdown terminates all M2.3 children. A crash relies on operating-
system PTY ownership and process-tree cleanup; the next application launch restores
only stopped placeholders.

### Tabs and split layout

The terminal workspace is:

```text
TerminalWorkspace
  tabs: ordered TerminalTab[]
  active_tab: TerminalTabId?

TerminalTab
  id
  name
  root: Pane | Split
  focused_pane: TerminalPaneId?

Split
  axis: horizontal | vertical
  ratio: integer basis points in [1000, 9000]
  first: Pane | Split
  second: Pane | Split
```

IDs are opaque and stable for the application lifetime and persisted presentation
state. Split creates a sibling adjacent to the focused pane. Close collapses a
single-child branch. At least one tab and one pane are created on the first explicit
terminal-open action; opening a workspace alone never spawns a process.

Tabs support create, activate, rename, close, and reorder-ready state. Panes support
focus, split horizontally, split vertically, restart, close, resize, select, copy,
paste, and explicit link opening. M2.3 does not expose drag reordering, but its state
does not prevent it later.

### Grid, cursor, attributes, and modes

The model uses a fixed visible grid sized in rows and columns plus a deque of
scrollback rows. Cells contain one grapheme starter, zero or more combining marks,
display width, foreground and background color, and semantic attributes. Wide
characters occupy a lead cell and a continuation cell; resizing and erasure never
leave orphan continuations.

M2.3 supports the commonly exercised terminal controls required by interactive
shells:

- printable Unicode and combining characters;
- carriage return, line feed, tab, backspace, and bell state;
- cursor movement, positioning, save/restore, visibility, and style;
- erase in line/display and insert/delete characters and lines;
- scrolling regions;
- SGR reset, bold, faint, italic, underline, inverse, strikethrough, 16 colors,
  256 colors, and true color;
- primary and alternate screen buffers;
- bracketed paste, application cursor keys, focus reporting, and mouse-reporting
  mode state;
- OSC title and OSC 8 hyperlink state with bounded payloads.

Unknown, malformed, oversized, and unsupported escape sequences are ignored with a
bounded diagnostic counter. They never allocate from an untrusted length or leave
the parser in an unbounded state.

### Input and clipboard

Native key events are mapped to terminal bytes using active terminal modes. Text
input uses Iced text events so composed Unicode is sent once. Platform command
shortcuts for copy, paste, pane navigation, tab navigation, split, close, and the
command palette are intercepted by the application and are not sent to the child.

Copy reads only the current terminal selection. Paste reads native clipboard text,
normalizes NUL out, applies bracketed-paste framing when enabled, and enforces a
configurable per-action byte limit before sending. Exceeding the limit requires an
explicit second action; M2.3 does not silently truncate a paste.

Links are detected from visible cell text and OSC 8 metadata. Activation selects a
link and shows its exact target. Opening is a separate explicit action through the
platform URL adapter. Only supported URL schemes are enabled; terminal output never
causes navigation on its own.

### Bounds and responsiveness

Each pane has independent limits:

- transport output queue: 4 MiB or 1,024 chunks, whichever is reached first;
- parser work per application tick: 256 KiB per pane and 1 MiB aggregate;
- default scrollback: 10,000 logical rows;
- maximum configured scrollback: 100,000 logical rows;
- maximum OSC/title/hyperlink payload: 8 KiB;
- maximum ordinary paste without confirmation: 1 MiB;
- renderer snapshot contains only visible rows plus the selected scrollback viewport.

When the transport queue is full, the reader blocks at that pane's queue boundary so
the operating-system PTY supplies backpressure. It does not drop arbitrary bytes,
because doing so could corrupt terminal state. The pane exposes a backpressure
indicator after 250 ms and a sustained-output warning after two seconds.

Application ticks drain panes round-robin. No pane receives a second parse quantum
until every ready pane receives one. Snapshot rebuilding is revision-based and
limited to rows changed since the previous snapshot where the renderer permits it.

### Native renderer

The Iced surface is a focused custom widget backed by the selected GPU renderer.
It consumes immutable `TerminalSnapshot` values and publishes normalized input,
selection, resize, focus, scroll, and link actions. It has no transport handle and
does not mutate the terminal domain directly.

The renderer uses a monospace font, cell-aligned glyph placement, semantic theme
tokens, clipped drawing, and explicit cursor geometry. The terminal palette derives
from terminal theme tokens rather than editor syntax tokens. A plain-text snapshot
view remains available to deterministic tests; it is not the production renderer.

### Persistence and restoration

The workspace contribution key is `terminal`. Schema version 1 persists:

- tab IDs, names, order, and active tab;
- the recursive split tree, axes, and ratios;
- pane IDs, focused pane, and last known working-directory hints;
- presentation profile ID and stopped status.

It never persists output, selection contents, clipboard contents, input history,
environment contents, executable arguments, exit output, child IDs, or handles.

On application restart every restored pane is a stopped placeholder labeled
`Stopped after application exit`. The user may explicitly restart one pane or all
panes in a tab. Restoration does not resolve or execute the previous command.
Unknown future fields remain opaque through the workspace contribution mechanism.

### Capability and module boundaries

The terminal capability is independently enableable. Disabling it:

1. confirms termination when panes are running;
2. terminates current children;
3. removes terminal commands and surfaces;
4. preserves its opaque workspace contribution unless explicitly reset;
5. leaves files, editing, search, and language state usable.

`strukt-terminal` must not depend on Iced, filesystem discovery, editor internals,
language-server internals, SSH, or persistence implementations. `strukt-app` wires
commands and events. `strukt-persistence` defines the schema representation and
atomic storage through the existing contribution mechanism.

## User Experience

The terminal drawer remains keyboard-accessible and can expand into the primary
canvas without changing process ownership. Opening it with no terminal shows a
single `New Terminal` action. Creating the first terminal starts the default shell
in the workspace root.

Terminal chrome always shows the local boundary, tab name, pane title, working-
directory hint, process state, and backpressure or truncation notices. An exited or
restored pane retains its place and offers `Restart` and `Close`.

The primary actions are available by keyboard and command palette. Pointer controls
remain visible but are not required for tabs, focus movement, split creation,
selection, copy, paste, restart, or close.

## Failure Handling

- **No workspace:** new terminal stays disabled and explains that a local workspace
  must be opened.
- **Shell resolution failure:** keep a stopped placeholder and allow profile repair
  or retry.
- **PTY/ConPTY spawn failure:** preserve the pane, display the exact adapter error,
  and offer retry without affecting another pane.
- **Reader or writer failure:** transition the pane to disconnected/exited state,
  retain parsed output, and stop accepting input.
- **Resize failure:** keep the last acknowledged size, show a pane-local warning,
  and retry only after another size change.
- **Parser limit or malformed escape:** ignore the bounded sequence, increment a
  visible diagnostic count, and continue parsing.
- **Noisy output:** apply pane-local queue backpressure and fair parse scheduling.
- **Clipboard unavailable:** preserve selection and report the platform error.
- **Unsafe or unsupported link:** display the target but disable open.
- **Persistence corruption:** use the last-valid workspace snapshot and restore no
  process automatically.
- **Renderer failure:** preserve the terminal process and model, replace the surface
  with an error view, and permit explicit termination.

## Security and Trust

- Opening a terminal runs only the user's resolved default shell or an explicitly
  selected user profile.
- Opening a workspace does not spawn a terminal.
- Workspace-provided commands and profiles require the shared trust model and are
  excluded until that model exists.
- Child environment inheritance is platform-standard but environment contents are
  never persisted or logged by the terminal module.
- Terminal output cannot open URLs, execute application commands, write files, or
  grant capabilities by escape sequence.
- OSC payloads, detected URLs, input, output queues, and scrollback are bounded.
- Paste is explicit and bracketed when the child requests bracketed-paste mode.
- Process termination targets the pane's child/process group through the transport
  adapter and never an unverified PID from persisted state.

## Verification Strategy

### Domain tests

- parser actions, Unicode width, combining marks, wide-cell invariants, wrapping,
  scrolling regions, alternate screen, colors, attributes, and malformed sequences;
- bounded scrollback, OSC payloads, link detection, selection, copy text, paste
  framing, and resize/reflow invariants;
- tabs, nested splits, collapse, ratios, focus movement, rename, restart, close, and
  stopped restoration;
- fair round-robin draining and queue/backpressure state.

### Transport contract tests

One shared suite runs against Unix PTY and Windows ConPTY implementations:

- spawn a deterministic child in a temporary working directory;
- observe stdout and stderr through the pseudo terminal;
- write Unicode input and observe the child response;
- resize and observe the child-reported size;
- collect exit status;
- terminate a long-running child and its pane-owned process tree;
- isolate two concurrent children.

### Integration and smoke tests

The deterministic `--terminal-smoke <fixture-root>` mode opens a local workspace,
creates two terminal panes, runs a repository-owned deterministic child mode through
the native PTY/ConPTY adapter, verifies Unicode IO, ANSI parsing, resize, independent
exit state, split layout, bounded output, and stopped-placeholder round-trip, then
exits zero with one exact marker. It never invokes the user's shell and never writes
workspace metadata.

CI runs the smoke on macOS, Ubuntu, and Windows. Hosted Windows is authoritative for
ConPTY. Manual macOS validation covers real shell input, IME, selection, clipboard,
links, resizing, splits, themes, output load, focus, and accessibility exposure.

### Responsiveness stress

A deterministic producer emits at least 64 MiB while a second pane performs
request/response IO. During the run, application reducer tests continue file and
editor actions. Evidence records total duration, maximum queued bytes, whether
backpressure became visible, and confirmation that the quiet pane and editor both
made progress.

The 64 MiB gate measures the platform-independent runtime scheduler with bounded
64 KiB chunks on every hosted OS. Native PTY/ConPTY smoke remains a separate
adapter gate: it proves real process IO, Unicode, ANSI, resize, isolation, exit,
and bounded chunks. Unix additionally carries the full 64 MiB native stream;
Windows caps native screen output at 1 MiB because ConPTY renders and may coalesce
console output rather than acting as a byte-transparent pipe. The Windows-native
contract and scheduler stress still run in the same hosted job.

## Delivery Plan Boundary

M2.3 is delivered as one reviewable terminal slice with internal commits for:

1. identifiers, grid, parser, bounds, selection, and links;
2. tabs, splits, pane lifecycle, persistence schema, and stopped restoration;
3. shared transport contract plus Unix PTY and Windows ConPTY implementation;
4. application commands, background event flow, and terminal drawer/canvas UI;
5. custom renderer, keyboard/pointer input, clipboard, links, and themes;
6. deterministic smoke, stress fixture, native walkthrough, review, and evidence.

M2.3 completion does not complete M2. Language intelligence and the final M2
integration/restoration gate remain separate slices.

## Acceptance Criteria

M2.3 is complete when:

1. A user can explicitly start a default local shell in the workspace root on
   macOS and Windows without enabling AI or a cloud service.
2. Multiple tabs and nested horizontal or vertical split panes run independently.
3. Unicode input, ANSI styles, cursor, selection, copy/paste, explicit links,
   resize, bounded scrollback, exit status, restart, and close are usable.
4. One noisy pane cannot prevent another pane, file navigation, or editing from
   making progress, and backpressure is visible.
5. Unix PTY and Windows ConPTY pass the shared native transport contract.
6. Workspace restoration recreates names and split layout as stopped placeholders
   without restarting any command or persisting output or environment contents.
7. Spawn, IO, resize, exit, termination, persistence corruption, parser limits,
   and renderer failure have pane-local recoverable behavior.
8. The terminal capability can be disabled without corrupting or disabling files
   and editing.
9. Local, hosted macOS/Ubuntu/Windows, manual macOS, stress, and full-slice review
   evidence is recorded.
10. Iced terminal input, selection, clipboard, resize, rendering, focus,
    accessibility, and sustained-output results are recorded without overstating
    unavailable human Windows validation.
11. The implementation does not absorb persistent sessions, SSH, remote terminals,
    tmux, shell integration injection, or automatic project command execution.
