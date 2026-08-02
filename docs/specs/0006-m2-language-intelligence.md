# M2.4 Language Intelligence

- Status: Approved design
- Date: 2026-08-02
- Milestone: M2 — Local Development Workspace
- Governing specs:
  - [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)
  - [`0003-local-development-workspace.md`](0003-local-development-workspace.md)
- Depends on:
  - M2.1 workspace and files
  - M2.2 native editor
  - M2.3 local terminals

## Summary

M2.4 adds language-agnostic editor intelligence through one generic Language
Server Protocol client. `strukt` discovers user-installed language servers,
starts them only when matching documents are open, synchronizes documents, and
normalizes diagnostics, completion, hover, and definition results into
UI-independent domain types.

Language-specific support is declarative data rather than Rust control flow.
Editing, saving, syntax highlighting, files, and terminals remain fully usable
when no server exists, a server is disabled, approval is withheld, the protocol
is malformed, or a server crashes.

The protocol contract follows the current official
[Language Server Protocol specification](https://microsoft.github.io/language-server-protocol/specifications/specification-current)
while deliberately implementing a small interoperable subset for the public
alpha.

## Goals

- Provide one language-agnostic client for standards-compliant LSP servers.
- Discover compatible executables already installed on the user's `PATH`.
- Represent language-specific discovery and launch behavior as data.
- Start one server per workspace and language only after a matching document opens.
- Support diagnostics, completion, hover, and go-to-definition.
- Keep requests cancelable and reject responses for stale workspaces, documents,
  revisions, positions, or server generations.
- Bound protocol input, stderr, queues, restart attempts, and shutdown time.
- Preserve the local-first and cloud-optional architecture.
- Define transport-neutral editor contracts that M4 can reuse remotely.
- Complete M2 integration and restoration validation without implicitly starting
  terminal or language-server processes.

## Non-goals

- Downloading, installing, updating, or bundling language servers.
- Language-specific client implementations or hard-coded protocol behavior.
- Executing a repository-provided server command without explicit approval.
- Rename, references, formatting, code actions, semantic tokens, inlay hints,
  signature help, workspace symbols, or debugging.
- Plugin-managed descriptors; M7 will expose the descriptor registry to plugins.
- Remote language-server execution; M4 will reuse the contracts over remote
  workspace transport.
- Persisting source text, diagnostics, completion items, hover text, protocol
  messages, stderr, or process identifiers.
- Claiming full IDE parity with specialist language extensions.

## Product Decisions

### Language-agnostic core

`strukt-language` contains no branching on Rust, TypeScript, Python, or another
language ID. A `LanguageServerDescriptor` supplies candidate executable names,
arguments, supported language IDs, workspace marker hints, and initialization
options. Descriptors may be added without changing lifecycle, protocol, editor,
or UI code.

Built-in public-alpha descriptors cover the languages already represented by the
editor grammar registry where a conventional standalone LSP executable exists.
An arbitrary user descriptor can provide the same fields. A descriptor advertises
compatibility; it does not imply that the executable is installed.

### Installation boundary

`strukt` searches the inherited user `PATH` and explicit absolute executable
paths. It may display descriptor-provided installation documentation or a copyable
command, but it never runs an installer, package manager, update command, shell
snippet, or downloaded binary.

### Trust boundary

A discovered executable from the user's inherited `PATH` may start automatically
after a matching document opens, subject to the descriptor being enabled.

An executable path or command supplied by workspace content is untrusted. Before
first execution, the interface shows the exact executable, arguments, resolved
path, and workspace root. Approval is scoped to the workspace identity plus the
canonical executable and arguments. Any change invalidates approval. Denial keeps
editing available and leaves the server in `ApprovalRequired` or `Disabled`.

Opening or restoring a workspace never executes a language server by itself.
Opening a matching document is the earliest automatic start point.

### Lifecycle boundary

There is at most one live language-server process for a `(workspace identity,
language ID)` pair. All matching open documents share it. The coordinator starts
the server on demand, reuses it across tabs, and begins an idle shutdown after the
last matching document closes. The idle delay is 30 seconds; reopening a matching
document cancels it without restarting the process. Workspace replacement and
application exit always request graceful shutdown and then enforce bounded
termination.

## Architecture

### Crate boundary

A new `strukt-language` crate owns:

- descriptor and discovery models;
- JSON-RPC message IDs, requests, responses, notifications, and errors;
- `Content-Length` framing over UTF-8 JSON;
- bounded stdio process transport;
- initialize, synchronization, request, cancellation, shutdown, and restart state;
- URI and position conversion;
- normalized diagnostics, completion, hover, and definition types;
- generation and revision guards;
- bounded in-memory protocol and stderr diagnostics.

The crate does not depend on Iced, application state, filesystem UI, editor UI,
terminal UI, or persistence implementations.

### Application coordinator

`strukt-app` owns a language coordinator that maps open editor documents to
language sessions. Blocking discovery, spawn, read, write, wait, and termination
work runs outside the Iced reducer. Reducer messages carry workspace identity,
document ID, document revision, server generation, and request ID so stale
completions can be discarded without mutating current state.

### Editor integration

The editor consumes immutable normalized language snapshots. It remains the source
of truth for document text, revisions, cursors, and selections. The language module
cannot mutate a document directly. Completion insertion becomes an ordinary editor
transaction; definition navigation opens through the existing confined document
workflow.

### Future remote transport

Discovery and process transport sit behind interfaces separate from protocol and
normalization. M4 may discover and run a server on the remote host while preserving
the same document synchronization and editor result contracts.

## Descriptor Model

Each descriptor has:

- stable descriptor ID and display name;
- one or more editor language IDs;
- ordered candidate executable names or an explicit absolute path;
- argument vector with no implicit shell;
- optional workspace marker names used only for ranking;
- JSON initialization options with a bounded serialized size;
- documentation URL and optional human-readable installation guidance;
- default enablement;
- source: built-in, user configuration, or workspace configuration.

Discovery resolves candidates without executing them. Results record the canonical
executable path and descriptor source. Candidate ordering is deterministic. An
explicit user selection wins, followed by a matching enabled built-in descriptor.
Workspace descriptors never outrank an approved user selection.

Unknown fields in persisted descriptor selection data survive round trips so later
versions and plugins can extend the model safely.

User descriptors live in the application-data configuration file
`language-servers.json`. A repository may provide an optional root-level
`.strukt-language.json`; `strukt` reads but never creates or edits that file. Both
formats use schema version 1 and the fields above. Configuration is limited to
256 KiB, must be UTF-8 JSON, and rejects duplicate descriptor IDs, empty language
sets, executable paths that are neither a bare `PATH` name nor absolute, shell
command strings, NUL bytes, and oversized initialization options. The workspace
file must be a confined regular file and may not be reached through a symbolic
link. Every descriptor from it remains subject to exact workspace approval.

## Server States

The normalized state machine is:

- `Unavailable`: no compatible executable was found;
- `ApprovalRequired`: a workspace-provided command has not been approved;
- `Disabled`: the descriptor or workspace-language pairing is disabled;
- `Discovering`: executable discovery is in progress;
- `Starting`: the process exists and initialization is pending;
- `Ready`: initialization completed and documents may synchronize;
- `Degraded`: the process is alive but a bounded nonfatal protocol issue occurred;
- `Restarting`: a bounded crash recovery attempt is pending;
- `Failed`: startup, protocol, or restart policy failed;
- `Stopping`: graceful shutdown or forced termination is in progress;
- `Stopped`: no process is live.

Every transition is generation-scoped. A late discovery, start, response, exit, or
shutdown completion from an older generation is ignored.

## Protocol Contract

### Framing and limits

The client uses JSON-RPC 2.0 over child stdin/stdout with LSP `Content-Length`
headers and UTF-8 JSON bodies. Header names are handled case-insensitively. Unknown
headers are ignored within the header budget.

Public-alpha limits are:

- 16 KiB maximum header block;
- 16 MiB maximum message body;
- 256 pending outbound messages;
- 256 outstanding requests per server;
- 4 MiB aggregate bounded stdout queue;
- 1 MiB bounded stderr ring buffer;
- 250 ms editor-change coalescing window;
- 10-second initialize timeout;
- 5-second ordinary request timeout;
- 2-second graceful shutdown timeout;
- three automatic restarts within ten minutes, followed by `Failed`;
- exponential restart delays of 250 ms, 1 second, and 4 seconds.

An oversized, malformed, duplicate-ID, or invalid-state message fails only its
server generation. The full body is never copied into an error string.

### Initialization and capabilities

The client sends `initialize` first with workspace folders, client identity, and
only capabilities that `strukt` implements. It records the server's advertised
text synchronization, completion, hover, definition, and position-encoding
capabilities. `initialized` follows a successful response.

Unsupported dynamic registrations are acknowledged or rejected according to the
protocol without changing implemented capability claims. Feature requests are
issued only when both client behavior and server capability allow them.

### Document synchronization

After readiness, each matching open text document sends `textDocument/didOpen`
with its current text, language ID, URI, and monotonic document version. M2.4 uses
full-document `textDocument/didChange` synchronization for correctness and a
bounded implementation surface; edits within the 250 ms window coalesce to the
latest full text and revision. Save emits `textDocument/didSave` only when the
server capability requests it. Close emits `textDocument/didClose`.

Binary, invalid UTF-8, metadata-only, missing, and truncated large-file previews
never synchronize. A user-authorized full large-file edit may synchronize only if
the serialized message stays within the body limit.

### Position encoding

The editor domain uses Unicode scalar offsets. LSP positions are converted at the
protocol boundary using the server-negotiated encoding. UTF-16 and UTF-8 are
supported; UTF-16 remains the compatibility default when no encoding is
negotiated. Conversion clamps neither malformed server positions nor ranges:
invalid positions are rejected and recorded as bounded protocol diagnostics.

Tests cover ASCII, combining marks, emoji, astral characters, CRLF, and mixed line
endings in both directions.

### Cancellation and stale results

Completion, hover, and definition requests receive unique monotonic IDs. A cursor
move, relevant edit, document close, workspace replacement, new request of the same
kind, timeout, server restart, or capability disablement sends `$/cancelRequest`
when possible and invalidates the local request guard.

A response is applied only when all of these still match:

- workspace identity;
- document ID;
- document revision;
- request kind and ID;
- requested position where applicable;
- server descriptor and generation.

Cancellation is advisory at the protocol layer; local invalidation is authoritative.

### Shutdown

Graceful shutdown sends `shutdown`, waits up to two seconds for its response, sends
`exit`, closes stdin, and waits for process exit. Timeout, workspace replacement,
or application teardown then invokes platform process termination. Process IDs and
handles are never restored.

## Language Features

### Diagnostics

M2.4 consumes push diagnostics from `textDocument/publishDiagnostics`. Results are
normalized into URI, range, severity, message, optional source, optional code, and
related locations. The client accepts only the current workspace, open synchronized
documents, and a current version when the server supplies one.

Diagnostics render as editor range markers and in a Problems pane grouped by file.
Selecting a problem opens the confined file and moves the cursor. Diagnostics
outside the workspace may be displayed as external locations but never open
without explicit confirmation.

Closing a document, stopping its server, replacing the workspace, or receiving an
empty current diagnostic set clears its markers.

### Completion

Completion is explicit through a keyboard shortcut or command action in M2.4;
server-advertised trigger characters may also request it after the editor change
coalescing boundary. Results normalize label, detail, kind, sort/filter text,
insert text or text edit, and documentation.

The menu is bounded to 200 items. Unsupported snippets are inserted as plain text
only after removing placeholder syntax safely; additional edits outside the active
workspace or current document are rejected. Applying a completion is one
revision-checked editor transaction and one undo boundary.

### Hover

Hover is requested explicitly by keyboard or a pointer settled for 400 ms. Plain
text and Markdown results normalize into a bounded presentation model. Markdown is
rendered without embedded HTML, images, scripts, remote resource fetching, or
automatic links. Hover content is capped at 256 KiB and disappears on edit, cursor
move, document close, focus loss, or server generation change.

### Definition

Definition is available by keyboard shortcut and command action. A single result
opens directly through the safe document workflow. Multiple results open a bounded
picker. Workspace files are confined and revision-safe. External `file:` locations
require confirmation; non-file URI schemes are displayed but not opened in M2.4.

The origin location remains available for one-step navigation back during the
current application session. Navigation history is bounded and not persisted.

## Interface

The editor status row shows language mode and one language-server state. Selecting
the state opens actions appropriate to that state:

- discover again;
- select a descriptor;
- enable or disable for this workspace and language;
- approve or deny the exact workspace command;
- restart a failed or ready server;
- copy bounded failure details;
- open descriptor documentation.

The Problems pane is a first-class workspace surface, not a modal dialog. It shows
error, warning, information, and hint counts; file grouping; current filtering;
and explicit navigation. It does not become the default center view merely because
a diagnostic arrives.

Completion and hover are transient editor overlays. Escape dismisses them before
affecting broader workspace state. Keyboard focus never leaks completion navigation
or descriptor editing into a terminal pane.

## Persistence and Privacy

Application data may persist:

- selected descriptor ID per workspace and language;
- enabled or disabled state;
- canonical approved workspace executable plus argument fingerprint;
- Problems-pane visibility and presentation preferences.

Application data never persists:

- document content or protocol payloads;
- diagnostics, completion, hover, or definition results;
- stderr or server logs;
- process identifiers, handles, pending requests, or restart counters;
- environment values or secrets.

Workspace-local `.strukt` metadata is not created. Approval records use the stable
workspace identity and invalidate when the canonical executable, arguments, or
workspace identity changes.

## Failure Handling

- Missing executable: report `Unavailable` with installation guidance; keep editing.
- Approval denied: do not spawn; keep language features disabled for that pairing.
- Spawn failure: report the exact bounded adapter error and preserve other servers.
- Initialize timeout/error: terminate that generation and apply restart policy.
- Malformed or oversized protocol input: fail that generation without retaining
  the body or affecting editor content.
- Stderr flood: retain only the newest bounded diagnostic bytes.
- Request timeout: cancel and clear only the affected transient feature.
- Crash: clear transient results, retain editor diagnostics only until the state
  visibly changes to restarting, then clear them before a new generation becomes
  ready.
- Crash loop: enter `Failed` after three automatic retries; require explicit restart.
- Workspace replacement: reject all stale work, clear language UI, and terminate all
  previous workspace servers outside the reducer.
- Persistence failure: keep current in-memory selection and approval state, report
  the error, and never fall back to workspace metadata.

## Capability Isolation

Language intelligence is registered as an independent capability. Disabling it:

- cancels requests;
- shuts down servers;
- clears transient language overlays and diagnostics;
- leaves files, editor text, saves, syntax highlighting, terminals, layout, and
  persistence usable;
- preserves opaque language contribution data for future re-enablement.

One failed descriptor or language pairing never disables another pairing.

## Testing and Verification

### Domain and protocol tests

- descriptor validation, deterministic matching, PATH discovery, and ranking;
- workspace-command approval fingerprints and invalidation;
- fragmented and combined `Content-Length` frames;
- case-insensitive headers, unknown headers, and every framing/body limit;
- JSON-RPC request/response/notification/error routing and duplicate IDs;
- initialize ordering, capability capture, and invalid-state rejection;
- full document open/change/save/close synchronization and coalescing;
- UTF-8/UTF-16 conversion across Unicode and line-ending cases;
- normalized diagnostics, completion, hover, and definitions;
- cancellation and every stale-result guard;
- bounded stderr, queues, timeouts, restarts, shutdown, and forced termination.

### Deterministic fake server

A repository-owned fake language server is built as a separate executable. It
supports deterministic modes for successful initialization, synchronization,
diagnostics, completion, hover, definition, cancellation observation, fragmented
frames, malformed messages, oversized declarations, stderr flooding, crashes,
shutdown, and delayed stale responses. Tests never require a third-party language
server or network access.

### Application tests

- matching document open schedules discovery but workspace restore alone does not;
- approval is required for workspace-provided commands and exact changes invalidate it;
- diagnostics and Problems navigation use confined document opening;
- completion applies one revision-safe transaction and undo boundary;
- hover and completion focus cannot leak into terminals;
- definition handles single, multiple, external, and unsupported URI results;
- workspace replacement and capability disablement discard stale work;
- files, saves, editor actions, and terminals continue after server failure;
- persisted selection, approval, and pane visibility restore without process start.

### Cross-platform smoke

`--language-smoke <existing-root>` launches only the repository fake server. It
verifies discovery, initialization, Unicode document synchronization, diagnostics,
completion, hover, definition, cancellation, shutdown, stopped restoration, exact
success output, and absence of `.strukt` metadata. CI runs it on macOS, Ubuntu, and
Windows with an outer timeout.

### Final M2 integration

A deterministic M2 integration smoke opens a workspace, edits and saves a file,
runs the language fake, operates independent terminal panes, persists presentation
state, restores it in a fresh application model, and proves:

- file, editor, terminal, language, and layout contributions coexist;
- noisy terminal and language work do not starve file/editor actions;
- restored terminals are stopped placeholders;
- restored language selection and Problems visibility do not start a process;
- no workspace-local metadata or runtime content is persisted;
- capability failures remain isolated.

The native macOS walkthrough covers visible diagnostics, Problems navigation,
completion, hover, definition, server status, approval, restart, theme contrast,
keyboard focus, and accessibility exposure. Hosted Windows is authoritative for
native process/protocol automation; human Windows visual, accessibility, and IME
certification remains an M9 public-alpha gate.

## Acceptance Criteria

1. One generic `strukt-language` client supports arbitrary valid descriptors
   without language-specific lifecycle or feature branches.
2. User-installed servers are discovered without execution; missing servers show
   guidance and are never installed by `strukt`.
3. A matching open document starts at most one approved server per workspace and
   language; workspace restore alone starts none.
4. Workspace-provided commands require exact, invalidatable approval before spawn.
5. Initialization and full document synchronization follow the server's advertised
   capabilities and negotiated UTF-8 or UTF-16 positions.
6. Diagnostics, completion, hover, and definition work through normalized,
   revision-safe editor contracts.
7. Requests are cancelable and stale discovery, process, response, diagnostic, and
   shutdown completions cannot cross workspace, document, revision, request, or
   server-generation boundaries.
8. Protocol bodies, headers, queues, stderr, timeouts, and restart attempts remain
   within documented bounds.
9. Missing, disabled, denied, malformed, crashed, or failed servers never block
   files, editing, saving, syntax highlighting, terminals, or other language servers.
10. Persistence contains only selection, enablement, exact approval, and presentation
    state; no source, results, logs, environment data, process state, or workspace
    metadata is written.
11. The deterministic fake-server and M2 integration smoke pass locally and in
    hosted macOS, Ubuntu, and Windows jobs.
12. Manual macOS results, hosted links, limitations, review findings, and direct
    acceptance evidence are recorded before merge readiness.

## Delivery Boundary

M2.4 is delivered as one reviewable language-intelligence and final M2 integration
slice with internal commits for:

1. descriptor, discovery, approval, and normalized domain types;
2. bounded JSON-RPC/LSP framing and protocol normalization;
3. process lifecycle, synchronization, cancellation, and restart policy;
4. diagnostics and Problems-pane integration;
5. completion, hover, and definition integration;
6. persistence, capability isolation, and stopped restoration;
7. fake-server smoke and final M2 integration validation;
8. review, evidence, tracker, roadmap, README, and ADR updates.

M2 is complete only after this slice proves the combined local workspace exit
criteria. M3 persistent local sessions begin afterward and must reuse terminal,
workspace, persistence, and language transport boundaries rather than coupling
session durability to the M2 ephemeral process model.
