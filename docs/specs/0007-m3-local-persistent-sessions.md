# M3 Local Persistent Sessions

- Status: Approved for implementation
- Date: 2026-08-02
- Milestone: M3 — Local Persistent Sessions
- Governing specs:
  - [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)
  - [`0003-local-development-workspace.md`](0003-local-development-workspace.md)
- Predecessor:
  - [`0005-m2-local-terminal.md`](0005-m2-local-terminal.md)
- Interaction reference:
  - [`../mockups/workspace-shell/remote-multiplexer.html`](../mockups/workspace-shell/remote-multiplexer.html)

## Summary

M3 turns local terminal panes into durable named sessions. A local session owns
windows, split panes, PTYs, bounded scrollback, and attention state independently
of the desktop UI. Closing `strukt` detaches the UI; reopening it reattaches to the
same live processes through a per-user local session service.

The session service is native and built into `strukt`. It does not require tmux.
The user-facing provider contract is deliberately reusable by the remote native
and tmux-backed providers planned for M5.

Machine restart is a separate safety boundary. Session definitions, names, window
layouts, working directories, and available bounded history may restore after a
reboot, but every pane is stopped. `strukt` never restarts an arbitrary command or
shell merely because the machine or application started.

## Goals

- Provide a session → window → pane hierarchy for local terminal work.
- Keep live PTYs running while every desktop window is closed.
- Reattach after application restart with current screen, bounded history, and
  lifecycle state.
- Keep multiple noisy sessions isolated and the UI responsive.
- Expose keyboard-first create, attach, detach, rename, duplicate, restart, close,
  and terminate actions.
- Persist only safe session definitions and presentation state in application data.
- Establish a provider and protocol boundary reusable by M5.
- Support macOS, Linux, and Windows from one domain model.

## Non-goals

- SSH transport, remote files, or remote helpers; those belong to M4.
- Persistence across a remote disconnect; that belongs to M5.
- tmux discovery or attachment; the provider is designed now and implemented in M5.
- Replaying arbitrary shell input or commands after reboot.
- A general operating-system service installer or privileged daemon.
- Multi-user or network-visible session sharing.
- Collaborative simultaneous terminal input from multiple desktop clients.
- Unbounded output history, full command-block semantics, or session replay export.

## User Model

The local workspace exposes a Sessions activity item and a compact session strip.
The hierarchy is:

```text
Local workspace
└── Named session
    └── Window
        └── Split pane / PTY
```

A session is an independent working context such as `backend`, `operations`, or
`scratch`. A window is a named tab within that session. Each window owns one split
tree and one focused pane. Existing M2 terminal tabs migrate to one local session;
the word `window` is the M3 domain name for the same visual level.

The active workspace may have many sessions, but only one session and one window
are presented in the center terminal surface at a time. Switching does not stop or
resize hidden sessions unnecessarily. Unread output and attention state remain
visible in the session list.

## Architecture

M3 adds a UI-independent `strukt-session` crate and one helper binary,
`strukt-sessiond`.

```text
strukt-app
  └── SessionProvider client
        └── authenticated local IPC
              └── strukt-sessiond
                    ├── session catalog and reducer
                    ├── PTY/ConPTY runtime
                    ├── bounded terminal models
                    └── atomic definition/history store
```

`strukt-session` owns:

- stable session, window, pane, provider, and generation identifiers;
- hierarchy validation and lifecycle transitions;
- provider capabilities and normalized snapshots;
- framed request, response, and event messages;
- sequence, lease, bounds, and reconnection rules;
- application-data persistence models.

`strukt-sessiond` owns every persistent PTY and all mutable live terminal models.
The desktop app never receives a process handle. It renders immutable snapshots,
sends bounded input and lifecycle requests, and reconnects after transport loss.

The M2 `strukt-terminal` parser, grid, layout primitives, and native transport stay
the source of truth. M3 moves live runtime ownership behind the provider rather
than creating a second terminal implementation.

## Identity and Hierarchy

Identifiers are random 128-bit values serialized as lowercase hexadecimal. They
are unique within a provider and never derived from names, paths, process IDs, or
array positions.

Each session has:

- stable ID and editable display name;
- provider ID and provider capabilities;
- one or more windows;
- active window ID;
- lifecycle state and revision;
- creation and last-activity timestamps;
- aggregate unread and attention state.

Each window has:

- stable ID and editable display name;
- one validated split layout;
- one or more pane IDs;
- focused pane ID;
- unread and attention state.

Each pane has:

- stable ID and generation;
- canonical working directory when it remains available;
- requested terminal size;
- stopped, starting, running, exited, failed, or backpressured state;
- bounded current screen and scrollback snapshot;
- monotonically increasing output revision;
- unread and attention state.

Names are trimmed Unicode strings of 1–80 scalar values. Generated names are
deterministic within the current catalog. Duplicate names are allowed and IDs are
always authoritative.

At most 64 sessions, 32 windows per session, 32 panes per window, and 256 total
panes may exist in one service. Layout depth is capped at 16.

## Local Service Lifecycle

The desktop app discovers a service rendezvous record in the per-user application
data directory. The record contains a protocol version, endpoint identity, service
instance ID, and authentication secret reference. It never lives in the workspace.

If no healthy service exists, the app may start the repository-owned helper only
after the user creates, restarts, or attaches a session. Opening or restoring a
workspace alone never starts the helper or a PTY.

The helper:

- runs without administrator or root privileges;
- accepts only local per-user IPC;
- rejects unauthenticated clients before catalog access;
- owns a single application-data directory through an exclusive service lock;
- inherits no terminal, stdin, stdout, or stderr handle from the UI;
- stays alive while at least one pane is running;
- may exit after 30 minutes with no running panes and no attached client;
- handles one controlling desktop client in M3 and rejects a conflicting writer.

Closing every `strukt` window sends `DetachClient`, not `TerminateSession`. The
service keeps live panes running. An explicit Quit and terminate-all action is
separate and requires confirmation when a pane is running.

If the service dies, the UI marks every attached session stale, preserves the last
rendered snapshot, and remains responsive. Reconnection discovers either the same
service instance or a new stopped catalog. A new instance cannot accept stale
responses or events from the previous instance.

## IPC and Authentication

The provider transport uses a platform-local endpoint abstraction:

- Unix-domain sockets on macOS and Linux;
- per-user named pipes on Windows.

No TCP port is opened. The endpoint and rendezvous record are private to the
current user. Unix files use owner-only permissions. Windows creates a pipe and
rendezvous file whose access control is restricted to the current logon user and
local system.

The first client frame includes protocol version, service instance ID, client
nonce, and proof of the 256-bit per-service secret. Secrets are generated from the
OS random source and rotated whenever a new service instance creates the endpoint.
They are never logged, persisted into workspace state, placed in process arguments,
or returned through diagnostics.

Frames use a 4-byte big-endian length followed by versioned CBOR. Limits are:

- 64 KiB for ordinary requests and responses;
- 1 MiB for catalog or pane snapshots;
- 256 KiB for one input write;
- 4 MiB of queued outbound data per client;
- 1,024 queued events.

Unknown message fields survive persistence where forward compatibility needs them;
unknown wire message kinds are rejected. Oversized, malformed, unauthenticated, or
out-of-order frames close only that client connection.

Every request has a monotonically increasing client request ID. Every catalog
mutation includes the expected catalog revision. Every pane event includes service
instance, session, window, pane, generation, and output revision. Stale responses
and events are ignored.

## Provider Contract

The normalized provider contract supports:

- `catalog` and `attach`;
- create, rename, duplicate, detach, terminate, and remove session;
- create, rename, activate, duplicate, and close window;
- split, focus, resize, write, restart, terminate, and close pane;
- snapshot and incremental event polling;
- capability and health inspection.

Capabilities are explicit flags. The native local provider advertises live
detach/reattach, hierarchy mutation, structured screen snapshots, bounded history,
attention, input, resize, and process termination. Future tmux providers may omit
structured history or atomic layout mutation, and the UI must disable unavailable
actions rather than pretending parity.

Provider errors are normalized and bounded: unavailable, authentication failed,
version incompatible, stale revision, capacity reached, invalid action, not found,
transport lost, process failed, and internal failure. Raw OS errors, environment
values, secrets, and protocol bodies never enter persisted state.

## Session and Window Actions

Creating a session creates one window and one stopped pane. The helper starts no
process until an explicit start/restart action. The default initial process is the
same platform shell resolution used by the M2 terminal.

Detaching removes the client view lease while preserving every process. Attaching
returns a bounded current catalog and the active window snapshot. The service does
not replay unbounded raw output.

Renaming changes presentation only. Duplicating a session copies names, working
directories, window hierarchy, split ratios, and sizes into a new stopped session.
It copies no process, input, environment, screen content, scrollback, or command.

Restarting a pane terminates its current generation, clears its terminal model,
and starts the default shell only after explicit confirmation when a process is
still running. Restarting a window or session is a batch of explicit pane restarts
with independent results; one failure never rolls back successful siblings.

Terminating a session gracefully terminates every pane with a bounded deadline and
then forces remaining children closed. Removing a stopped session deletes only its
definition and bounded history. Closing the last window removes the stopped
session or asks for terminate confirmation when any pane is live.

## Output, Backpressure, and Attention

The service applies the existing terminal parser and scrollback bounds. It drains
panes round-robin with per-pane and aggregate budgets. One noisy pane cannot starve
input, lifecycle requests, another pane, or service heartbeats.

The service exposes immutable structured snapshots rather than raw historical byte
streams. A snapshot contains the visible grid, a bounded scrollback window, cursor,
title, modes needed for input, pane state, and output revision. It never includes
environment values or a command history.

When the client is attached, changed panes emit coalesced revision notifications;
the client requests the newest snapshot. Intermediate revisions may be skipped.
When detached, output remains bounded by the same per-pane scrollback cap.

Output received while a session or window is not active increments unread state.
Bell, supported urgent terminal signals, process exit, failure, and backpressure
set attention state. Activating and viewing the newest pane revision clears unread;
attention clears only by viewing or explicit acknowledgement.

## Persistence and Reboot Safety

The service atomically persists a versioned catalog in application data after
debounced mutations. It may persist:

- IDs, names, hierarchy, split ratios, active/focused IDs, and sizes;
- canonical working directories;
- stopped lifecycle definitions and last exit status;
- bounded sanitized screen/scrollback snapshots;
- unread and attention presentation state;
- provider and schema metadata with unknown forward-compatible fields.

It never persists:

- terminal input or command history;
- arbitrary environment variables or secrets;
- process IDs, handles, OS tokens, pipe names, or authentication secrets;
- executable arguments typed inside a shell;
- clipboard data or selected text;
- unbounded output or raw transport frames.

After application restart with a surviving helper, the live catalog is
authoritative. After service or machine restart, the persisted catalog loads with
every pane in `Stopped`; prior output is labeled historical. Working directories
that no longer exist fall back visibly to the workspace root only after an
explicit restart action.

The service is never registered for OS login launch in M3. Merely opening `strukt`
after reboot does not start it.

## Interface and Commands

The Sessions activity surface lists named sessions with provider, lifecycle,
window count, unread count, and attention state. The center session strip stays
compact and preserves the M2 file browser. The file browser remains one shortcut
away and is never replaced merely because a session emits output.

The command palette and keyboard model expose scriptable commands with stable
names and ID arguments:

- `session.new`, `session.attach`, `session.detach`, `session.rename`;
- `session.duplicate`, `session.restart`, `session.terminate`, `session.remove`;
- `session.next`, `session.previous`;
- `window.new`, `window.rename`, `window.next`, `window.previous`, `window.close`;
- existing pane split, focus, resize, restart, and close commands under the active
  window.

Destructive actions name the exact session/window/pane and running count. Keyboard
focus cannot leak terminal input into session controls or vice versa. All states
use semantic theme tokens and remain distinguishable without color alone.

## Failure Handling

- Service absent: show stopped definitions and an explicit Start service action.
- Authentication failure: close the connection, rotate only through a verified
  stale-service recovery path, and never expose the secret.
- Version mismatch: keep M2 ephemeral terminals usable and show upgrade guidance.
- Transport loss: freeze the last snapshot, mark stale, reconnect with bounded
  exponential backoff while the app is open, and keep files/editor responsive.
- Stale mutation: refresh the catalog and require the user action to be retried.
- Pane spawn/write/resize failure: fail only that pane generation.
- Output pressure: mark only that pane backpressured and keep bounded newest state.
- Corrupt current catalog: fall back to the last valid catalog; otherwise start an
  empty stopped catalog and retain the corrupt file for explicit diagnostics.
- Service lock conflict: attach to the verified owner or fail closed; never run two
  writers against one store.
- Shutdown timeout: force only explicitly terminated panes; detach never kills.

## Migration from M2

On first M3 use, a restored M2 terminal contribution migrates into one stopped
session named `Local`. Each M2 terminal tab becomes a window with the same name,
layout, pane IDs when valid, working directories, and focus. No process starts.

Migration is idempotent and versioned. If both an M3 catalog and an old M2
contribution exist, the M3 catalog wins and the old contribution remains opaque
until the next successful workspace persistence write removes only the obsolete
terminal contribution.

## Testing and Verification

### Domain tests

- hierarchy IDs, caps, names, layout depth, and focused/active invariants;
- every lifecycle transition and stale revision/generation rejection;
- duplicate copies definitions but no process, input, history, or runtime state;
- capability-gated actions and normalized bounded errors;
- unread and attention transitions;
- M2 migration and forward-compatible persistence.

### Protocol and security tests

- fragmented/combined frames and every message/queue limit;
- authentication proof, secret rotation, wrong-user endpoint denial, and no secret
  in arguments/logs/state;
- request IDs, catalog revisions, service instances, generations, and output
  revisions reject stale work;
- malformed clients cannot affect a live session;
- local endpoint creates no listening TCP port.

### Native service tests

A repository-owned session fixture launches the real helper in isolated
application data. It creates two sessions, starts panes, writes distinctive output,
detaches every client, proves processes continue, reconnects, verifies output and
layout, exercises rename/duplicate/restart/terminate, and shuts the helper down.

Crash and restart tests kill the helper, load the persisted catalog into a new
instance, and prove every pane is stopped and no arbitrary command restarts.
Concurrent noisy panes verify fairness, queue bounds, and lifecycle responsiveness.

### Application tests

- opening/restoring a workspace alone starts no helper or PTY;
- creating or attaching a session schedules service work outside the reducer;
- session/window/pane projections preserve file, editor, language, and terminal
  focus isolation;
- transport loss marks stale without blocking workspace actions;
- session and window actions are keyboard reachable and capability gated;
- M2 stopped layouts migrate without process start;
- theme tokens distinguish active, live, stopped, stale, unread, and attention.

### Cross-platform smoke

`--session-smoke <existing-root>` launches only repository-owned binaries with
isolated application data. It creates two sessions, starts independent terminal
fixtures, detaches, reconnects, verifies exact output and layout, terminates one
session without affecting the other, restarts the service, verifies stopped-only
restoration, prints one exact success marker, and leaves the workspace free of
`.strukt` metadata.

CI runs format, strict Clippy, full tests, native builds, and the smoke on macOS 14,
Ubuntu 24.04, and Windows Server 2022 with an outer timeout.

### Manual validation

The macOS native walkthrough covers session creation, switching, windows, splits,
detach/reattach after closing the UI, unread/attention, destructive confirmation,
service-loss presentation, file-browser accessibility, keyboard focus, themes,
and no workspace metadata. Hosted Windows service automation is required; human
Windows visuals, accessibility, and IME remain public-alpha release gates.

## Acceptance Criteria

1. Multiple named local sessions own independent windows, panes, layouts, PTYs,
   bounded history, and attention state.
2. Closing the desktop UI detaches without terminating running sessions.
3. Reopening the app reattaches to a surviving service and restores current screen,
   bounded history, layout, and lifecycle state.
4. A service or machine restart restores stopped definitions and never restarts an
   arbitrary shell or command automatically.
5. Create, attach, detach, rename, duplicate, restart, terminate, and remove actions
   are keyboard reachable, scriptable, capability gated, and revision safe.
6. The service uses authenticated per-user local IPC with no network listener,
   workspace metadata, privileged installation, or leaked secret.
7. One noisy or failed session cannot starve or corrupt another session, workspace
   files, editor, language server, or the desktop UI.
8. Persistence contains only bounded definitions, presentation, and sanitized
   history; never input, command history, environment secrets, or process handles.
9. M2 terminal layouts migrate into one stopped local session without process start.
10. The provider contract expresses native local capabilities without hard-coding
    tmux or remote transport into the UI.
11. Deterministic service and application smokes pass locally and in hosted macOS,
    Ubuntu, and Windows jobs.
12. Manual evidence, agentic review, limitations, and exact merge-head CI are
    recorded before M3 is marked complete.
