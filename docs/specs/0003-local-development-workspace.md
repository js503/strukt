# M2 Local Development Workspace

- Status: Approved
- Date: 2026-07-29
- Parent spec:
  [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)
- Architecture decision:
  [`../decisions/0001-native-ui-framework.md`](../decisions/0001-native-ui-framework.md)
- First implementation plan:
  [`../plans/0003-m2-workspace-files.md`](../plans/0003-m2-workspace-files.md)
- First-slice tracking issue:
  [#3 — M2: Local workspace and files](https://github.com/js503/strukt/issues/3)
- Spatial reference:
  [`../mockups/workspace-shell/focus-context.html`](../mockups/workspace-shell/focus-context.html)

## Summary

Milestone M2 turns the M1 native shell into a real local development workspace. A
developer can open a local folder, browse and search its files, edit and save source
code, use configurable language-server features, run multiple local terminals, and
restore the workspace after restarting `strukt`.

The milestone remains local-first and cloud-independent. Opening a folder does not
modify the repository, enable AI, execute project code, or require a `strukt`
account. Personal layout, editor, language, and terminal presentation state lives
in platform application data.

M2 establishes the local contracts that later milestones reuse for persistent
sessions and SSH-backed workspaces. It does not make local terminal processes
persistent; M3 adds durable process ownership and detach/reattach behavior.

## Goals

- Make a local folder a usable `strukt` workspace without creating repository
  metadata.
- Keep the file explorer immediately accessible and capable of revealing every
  accessible file.
- Provide responsive file navigation, Quick Open, search, editing, saving, and
  external-change handling.
- Provide an IDE-level editor with a language-agnostic language-server client.
- Support multiple local terminal tabs and splits through shared PTY/ConPTY
  contracts.
- Restore workspace layout, editor state, and stopped-terminal placeholders after
  an application restart.
- Keep filesystem, editor, terminal, language, and persistence behavior outside the
  Iced application crate.
- Prevent background indexing, language servers, or terminal output from blocking
  unrelated UI work.
- Validate the Iced framework against real text editing, IME, accessibility, focus,
  custom terminal rendering, and sustained output.
- Operate without AI, cloud services, or a hosted control plane.

## Non-goals

- Persistent terminal processes that survive an application exit or machine reboot.
- Named persistent sessions, detach/reattach, or session-provider interoperability.
- SSH, remote filesystems, remote terminals, or the remote helper.
- Git visualization, task runners, debugging, containers, or Kubernetes surfaces.
- Collaboration, cloud synchronization, or account-based workspace sync.
- A repository-owned `.strukt` manifest.
- Multi-root workspace files.
- Bundling, downloading, or managing language-server installations.
- Automatically executing project tasks, shell hooks, or workspace-provided
  commands.
- Full language-specific IDE parity with established specialist editors.
- Human Windows packaging and visual certification, which remains an M9 gate.

## Decisions

### Folder-based workspace

Opening a folder creates a workspace identified by its normalized local path.
`strukt` does not create a `.strukt` directory or other project file.

Personal workspace state is stored in the platform application-data location and is
keyed by a stable identifier derived from the normalized path plus platform volume
identity where available. Moving a folder creates a new workspace identity unless
the user explicitly relinks it from recent workspaces.

The identity model must permit a future workspace target to be local or remote, but
M2 implements only one local root.

### Contract-first feature modules

M2 behavior is implemented in focused Rust modules behind typed contracts. Iced
renders immutable view state and emits commands; it does not own filesystem,
document, language-server, terminal, or persistence behavior.

Modules communicate through typed commands and events. A module cannot mutate
another module's internal state. Platform-specific code implements shared contracts
for filesystem watching, process lifecycle, Unix PTY, and Windows ConPTY behavior.

### IDE-level, language-agnostic editing

The editor provides source-editing fundamentals and consumes normalized language
features from a generic Language Server Protocol client. No language ecosystem is
required for the protocol design or acceptance tests.

Common syntax grammars for Rust, JavaScript, TypeScript, Python, JSON, TOML,
Markdown, and shell files ship as data-backed editor support. Syntax support does
not imply that a matching language server is installed.

### Multiple ephemeral terminals

M2 supports multiple terminal tabs and splits. Each pane owns one local PTY or
ConPTY process for the lifetime of the application. Processes end when `strukt`
exits.

On restart, the workspace restores terminal names, tab and split layout, last known
working directories, and stopped placeholders. Commands and processes never restart
automatically.

## User Experience

### Opening and restoring a workspace

The welcome view provides `Open Folder` and recent-workspace actions. Opening a
folder:

1. resolves and validates the selected path;
2. creates or loads its local workspace record;
3. renders the workspace immediately with a responsive shell;
4. starts bounded background discovery and watching;
5. restores the last focused view, open documents, layout, and stopped-terminal
   placeholders when prior state exists.

A missing or inaccessible recent folder remains visible with `Locate`, `Remove`,
and `Retry` actions. It does not prevent other workspaces from opening.

### File explorer

The explorer is pinned by default and remains reachable from the activity rail,
command palette, and platform shortcut.

It supports:

- expandable folder navigation;
- keyboard traversal and type-ahead selection;
- create, rename, move, duplicate, and delete actions;
- reveal in the operating-system file manager;
- copy path and copy relative path;
- visible local-workspace identity;
- resize, collapse, and restore;
- separate `Show Hidden Files` and `Show Ignored Files` controls.

Ignore rules affect default discovery, not authorization. Every accessible file can
be reached through an explicit explorer toggle, direct path, or search override.
Ignored and generated entries use muted visual treatment when shown.

Deletion uses the platform trash or recycle bin when available. Permanent deletion
requires an explicit destructive action and confirmation.

### Quick Open and search

Quick Open and workspace search honor:

- `.gitignore` and nested Git ignore files;
- platform hidden-file conventions;
- user and workspace exclude patterns;
- built-in heavy-directory defaults for `.git`, `target`, `node_modules`, and
  equivalent dependency or build caches.

Users can include hidden or ignored files for one operation or persist the choice
for the workspace. Direct path opening bypasses discovery exclusions.

Search is cancelable, streams bounded results, and reports incomplete or truncated
results. Binary files are excluded from content search by default.

### Editor

The center canvas supports multiple document tabs. The M2 editor provides:

- Unicode text input and IME composition;
- cursor movement, selection, rectangular-selection-ready primitives, and
  multi-cursor-ready edit operations;
- undo and redo with document-scoped history;
- line numbers, indentation, bracket matching, and whitespace controls;
- find, replace, match navigation, and case or regular-expression modes;
- syntax highlighting through the grammar registry;
- dirty-state and external-change indicators;
- manual save by default and configurable delayed autosave;
- diagnostics, completion, hover, and go-to-definition through the language module;
- keyboard navigation and accessible focus behavior.

M2 need not expose every multi-cursor command, but its edit model must not require a
rewrite to add them.

Binary files open in a safe metadata or preview view. Files beyond a configurable
size threshold open in bounded read-only mode with an explicit override. The
application must not allocate memory proportional to an unbounded file without
consent.

### External file changes

Filesystem events are normalized and correlated with open documents.

- A clean buffer reloads automatically when its file changes externally.
- A dirty buffer remains untouched and displays a conflict notice.
- Conflict actions include `Compare`, `Reload from Disk`, and `Keep Editing`.
- Deleted or moved files remain recoverable as dirty buffers until resolved.
- A formatter or save action does not create an external-change loop.

No watcher or background task mutates an editor buffer directly. The editor applies
an explicit normalized document event.

### Language intelligence

The language module provides a language-agnostic LSP client and normalized editor
events.

Language servers are found through:

- declarative discovery descriptors for known executables on `PATH`;
- user-level configuration;
- workspace-level configuration stored in application data.

The protocol core does not contain language-specific behavior. Discovery descriptors
are data that can be added independently of the LSP lifecycle.

`strukt` never downloads a language server. A discovered local executable is shown
in language status and may be enabled or disabled. A workspace-provided executable
command does not run until the workspace is trusted.

M2 language features include:

- initialization and capability negotiation;
- document open, change, save, and close synchronization;
- diagnostics;
- completion;
- hover;
- definition navigation;
- request cancellation;
- visible server status, logs, restart, and disable actions.

Editing and syntax highlighting continue when no language server exists or when a
server fails.

### Local terminals

Users can create, rename, close, restart, tab, and split terminal panes.

New terminals:

- use the platform's default user shell unless a profile overrides it;
- start in the workspace root;
- may inherit a sibling pane's current directory when reliable shell integration
  reports it;
- do not run a workspace command automatically.

Terminal interaction includes:

- Unicode input;
- ANSI colors and styles;
- cursor rendering;
- mouse and keyboard selection;
- copy and paste;
- clickable detected links with an explicit open action;
- resize propagation;
- bounded scrollback;
- exit status and restart actions.

An exited pane retains its exit status and in-memory scrollback until the pane is
closed or restarted.

## Architecture

### Module ownership

The intended ownership boundaries are:

- `strukt-core`: capability identifiers, typed command and event foundations, and
  cross-feature identifiers;
- `strukt-workspace`: workspace identity, lifecycle, aggregate state, and
  contribution coordination;
- `strukt-fs`: file metadata, ignore resolution, directory enumeration, file
  operations, watching, Quick Open discovery, and search;
- `strukt-editor`: document buffers, edits, selections, history, dirty state, and
  editor-facing view state;
- `strukt-language`: grammar registry, language-server discovery, LSP lifecycle,
  protocol normalization, and language features;
- `strukt-terminal`: terminal process contract, terminal grid and parser, bounded
  scrollback, terminal layout, and renderer-facing state;
- `strukt-persistence`: versioned schemas, migration, atomic snapshots, recovery,
  and platform storage locations;
- `strukt-app`: Iced application wiring, native widgets, layout, keyboard input,
  and rendering;
- platform-adapter implementations: filesystem watching, process lifecycle, Unix
  PTY, Windows ConPTY, trash or recycle-bin integration, and protected key storage.

Exact crate splits may combine a small contract and its default implementation when
the ownership boundary remains testable and does not introduce Iced into domain
crates.

### Command and event flow

The application follows one-way state flow:

1. a native input or background adapter emits a typed event;
2. the owning feature normalizes and validates it;
3. workspace coordination updates aggregate state or emits a command;
4. feature view state is published to `strukt-app`;
5. Iced renders the new state.

Long-running work returns through events and never holds a UI-thread lock.

Representative flows are:

```text
filesystem adapter
  -> file event
  -> file/workspace state
  -> document event
  -> editor and language synchronization
  -> view state

terminal input
  -> terminal command
  -> PTY/ConPTY adapter
  -> bounded output queue
  -> terminal parser and grid
  -> renderer state

editor change
  -> document event
  -> language request or notification
  -> normalized language result
  -> editor view state
```

### Responsiveness and backpressure

Filesystem discovery, content search, syntax parsing, language-server IO, PTY IO,
and persistence run outside the UI thread.

Each source has independent cancellation and bounds:

- directory discovery yields bounded batches;
- search streams capped result batches;
- watchers coalesce duplicate events and trigger bounded rescans after overflow;
- language requests are cancelable and stale responses are discarded;
- each terminal pane has independent input, output, and scrollback limits;
- persistence coalesces snapshots and never blocks an edit on disk IO.

Backpressure affects only the producing feature. A noisy terminal cannot block file
navigation, editing, or another terminal.

### Terminal engine

Terminal behavior is divided into:

- transport: spawn, input, output, resize, exit, and terminate;
- emulation model: ANSI parsing, grid, cursor, selection, links, and scrollback;
- native renderer: a focused GPU-backed custom Iced widget;
- layout: terminal tabs, splits, names, focus, and actions.

Unix PTY and Windows ConPTY adapters implement the same transport contract. The
terminal model and renderer do not depend on platform process APIs.

### Persistence

Workspace state is stored in a versioned local schema. Persisted state includes:

- workspace identity and root;
- enabled feature contributions;
- panel, tab, and split layout;
- active and open document paths;
- editor cursor, selection, scroll, and view state;
- language-server selections and non-secret settings;
- terminal names, layout, last known working directory, and stopped status;
- theme and workspace preferences.

Terminal output, shell environment contents, credentials, private keys, and running
process handles are not persisted.

Persistence uses atomic replacement and retains the last valid snapshot. Unknown
feature payloads remain opaque so disabling a feature does not make the workspace
unreadable.

Unsaved-buffer crash recovery is separately controllable. Recovery content is
enabled only when a platform-protected per-user key is available. Otherwise it is
disabled with a visible explanation. Disabling recovery deletes saved recovery
content.

## Failure Handling

- **Folder missing or inaccessible:** retain the recent entry and offer `Locate`,
  `Retry`, and `Remove`.
- **File permission failure:** report the affected operation and keep unrelated
  workspace features available.
- **Watcher overflow:** mark file state stale, perform a bounded rescan, and clear
  the indicator after reconciliation.
- **External dirty-buffer conflict:** preserve the buffer and require an explicit
  conflict action.
- **Search limit reached:** return partial results with a visible truncation state.
- **Persistence corruption:** load the last valid snapshot and retain the damaged
  file for diagnosis.
- **Language-server failure:** retain editing, expose status and logs, and use
  bounded restart attempts.
- **PTY spawn failure:** keep the terminal placeholder and expose the error and retry
  action.
- **Terminal process exit:** retain exit status and in-memory scrollback.
- **Noisy terminal:** truncate or apply backpressure within that pane and expose the
  condition.
- **Renderer or custom-widget failure:** isolate the affected surface where
  possible; never corrupt workspace state.

## Security and Trust

Opening a folder grants filesystem access to that root for explicit workspace
features. It does not grant permission to execute project code.

Before a workspace-provided language-server command, task, shell hook, or executable
configuration runs, `strukt` requires an explicit trust decision identifying the
command and scope. Revoking trust stops future automatic execution.

Symlinks that resolve outside the workspace root are identified visually and are
excluded from background indexing by default. Users may navigate them explicitly.

Workspace persistence does not store credentials, shell environment secrets,
terminal output, or private keys. Sensitive platform keys use native credential
storage. Local recovery data is user-scoped and protected as described in the
persistence section.

Destructive file operations identify their exact target. Trash or recycle-bin
operations are preferred; permanent deletion requires explicit confirmation.

## Iced Revalidation Gates

M1 accepted Iced for the native shell foundation, not unconditionally for every
future custom surface. Before M2 accepts its editor and terminal implementation,
evidence must cover:

- editable text and IME behavior;
- accessible labels and focus order;
- complete keyboard traversal;
- editor selection, clipboard, and large-document behavior;
- terminal input, selection, clipboard, Unicode, and resize behavior;
- custom-widget rendering and event routing;
- sustained terminal output while editor and file interactions remain responsive;
- macOS and Windows native behavior through the available manual and hosted gates.

If Iced fails a gate that cannot be corrected without compromising the domain
boundaries, retain the domain crates and evaluate Floem or a focused `winit`/`wgpu`
shell as required by ADR 0001.

## Verification Strategy

### Unit tests

Unit tests cover:

- workspace identity and lifecycle;
- ignore-rule precedence and visibility overrides;
- document edits, history, dirty state, and external-change reduction;
- syntax token application and language-feature normalization;
- terminal parsing, grid behavior, selection, links, and bounded scrollback;
- terminal tab and split layout;
- persistence serialization, migration, opaque payload retention, and recovery;
- command and event reducers.

### Contract tests

Shared contract suites cover:

- directory enumeration, metadata, file operations, atomic save behavior, and
  filesystem watching;
- Unix PTY and Windows ConPTY spawn, IO, resize, exit, and termination;
- platform trash or recycle-bin behavior;
- platform-protected recovery-key availability and lifecycle.

### Integration tests

Integration tests cover:

- opening and restoring a real temporary workspace;
- external changes to clean and dirty buffers;
- file create, rename, move, delete, and recovery flows;
- ignored-file search and explicit inclusion;
- deterministic LSP initialization, synchronization, diagnostics, completion,
  hover, definition, cancellation, crash, and bounded restart;
- multiple isolated terminal panes;
- high-volume output with concurrent editor and file actions;
- application restart with document, layout, and stopped-terminal restoration.

### Platform validation

CI runs formatting, linting, tests, and native builds on macOS, Windows, and Linux.
Native jobs include real Unix PTY or Windows ConPTY smoke processes.

Manual macOS evidence covers workspace opening, file editing, IME, accessibility,
keyboard traversal, editor interaction, terminal interaction, theming, and resizing.
Hosted Windows automation covers native file, language, ConPTY, and application
smoke paths. Human Windows packaging, visual, installation, and complete keyboard
workflow validation remains required for M9.

### Stress validation

Synthetic large-workspace and high-output fixtures verify that:

- discovery and search remain cancelable;
- result and event queues remain bounded;
- terminal output cannot prevent editor input or file navigation;
- one noisy terminal cannot starve another pane;
- persistence does not block active editing;
- truncation and degraded states are visible rather than silent.

Concrete fixture sizes and timing budgets belong in the implementation plan and
must be recorded with the validation evidence.

## Delivery Slices

M2 is implemented through separately reviewable slices:

1. **Workspace and files:** folder lifecycle, explorer, ignore behavior, Quick Open,
   search, watching, and persistence foundation.
2. **Editor:** document model, tabs, editing, save, external changes, syntax, IME,
   accessibility, and recovery.
3. **Local terminal:** PTY/ConPTY contracts, terminal model, renderer, tabs, splits,
   backpressure, and stopped placeholders.
4. **Language intelligence:** discovery descriptors, configuration, generic LSP
   lifecycle, diagnostics, completion, hover, and definition.
5. **Integration and restoration:** complete layout, state restoration,
   responsiveness, cross-platform evidence, and Iced revalidation.

Completing an individual slice does not make M2 complete.

## Acceptance Criteria

M2 is complete when:

1. A user can open a local folder as a workspace without creating repository
   metadata.
2. Reopening the workspace restores its focused view, open documents, editor state,
   layout, and stopped-terminal placeholders.
3. The explorer can reveal every accessible file while Quick Open and search honor
   ignore rules by default.
4. A user can explicitly include hidden and ignored files in exploration and
   search.
5. A user can edit and save source files with undo, redo, find, syntax highlighting,
   dirty-state reporting, and safe external-change handling.
6. IME, keyboard navigation, focus, and accessibility gates are recorded for the
   editor.
7. A user can configure an arbitrary standards-compliant language server and use
   diagnostics, completion, hover, and go-to-definition.
8. Editing remains usable when a language server is absent, disabled, or failed.
9. A user can create and operate multiple local terminal tabs and splits on macOS
   and Windows through the shared transport contract.
10. Terminal output, including a noisy pane, does not make file or editor
    interactions unresponsive.
11. Restarting `strukt` never restarts terminal commands automatically.
12. The complete local workspace operates without AI, cloud services, or a hosted
    account.
13. Applicable macOS, Windows, and Linux automated gates pass and validation
    evidence is recorded.
14. ADR 0001 records the M2 Iced revalidation result before the milestone is marked
    complete.

## Related Milestones

- M1 provides the native shell, capability registry, semantic theme tokens, and
  cross-platform CI foundation.
- M3 adds durable local session ownership and detach/reattach on top of M2 terminal
  contracts.
- M4 implements remote workspaces using the workspace, filesystem, language, and
  terminal boundaries proven here.
- M5 combines the persistent-session and remote-workspace models.
- M9 supplies human Windows packaging and public-alpha readiness gates.
