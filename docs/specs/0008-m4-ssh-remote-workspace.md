# M4 SSH Remote Workspace

- Status: Approved
- Date: 2026-08-02
- Governing spec: [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)
- Roadmap: [`../roadmap.md`](../roadmap.md)
- Interaction reference: [`../mockups/workspace-shell/remote-workspace.html`](../mockups/workspace-shell/remote-workspace.html)

## Summary

M4 makes a standard SSH-accessible Linux development box a first-class `strukt`
workspace. A user chooses an OpenSSH host alias and a remote directory, connects
with the keys, agent, known-host policy, includes, and jump-host configuration they
already use, and works with remote files, Quick Open, editing, search, Git status,
approved tasks, diagnostics, language intelligence, and an ephemeral terminal in
the native workspace shell.

The desktop application delegates SSH transport and policy to the platform
OpenSSH client. A small versioned `strukt-remote` helper may be installed under the
remote user's home directory with explicit consent. It runs only with that user's
permissions, listens on no network port, and communicates over one SSH stdio
channel. If the helper is missing, incompatible, or unhealthy, the workspace keeps
a usable direct SSH terminal and exposes repair or retry actions.

M4 does not make remote terminal processes persistent. M5 will reuse this transport
and helper foundation to keep multiple remote session/window/pane hierarchies alive
through disconnect and local app restart and to add tmux interoperability.

## Approved Design Decision

`strukt` uses the installed OpenSSH executable through a typed Rust adapter rather
than implementing SSH or linking a separate SSH policy stack for the public alpha.
This preserves the user's effective OpenSSH configuration and security behavior,
including agent selection, `known_hosts`, `Include`, certificates, and
`ProxyJump`, on macOS, Windows, and Linux.

The adapter never constructs a shell command string. It invokes `ssh` with a
validated argument vector and passes the host alias as one opaque argument. Remote
helper bootstrap commands are fixed, versioned scripts owned by `strukt`; user
input is transferred as framed stdin data rather than interpolated into those
scripts.

## Goals

- Open a Linux development box, including an EC2 instance, through a standard SSH
  host alias.
- Preserve normal OpenSSH config, agent, key, host verification, and jump-host
  behavior without copying private credentials into `strukt`.
- Keep the remote execution boundary visible in every workspace surface.
- Make remote files and Quick Open as immediate as their local counterparts.
- Reuse editor, diagnostics, language, terminal-rendering, task-approval, and theme
  contracts instead of building a parallel remote UI.
- Keep the application responsive through disconnect, reconnect, slow operations,
  helper failure, and noisy process output.
- Provide a useful direct terminal when the helper cannot run.
- Establish a versioned, bounded remote protocol that M5 can extend without
  coupling SSH behavior to the UI.

## Non-goals

- Persistent remote sessions, missed terminal output after disconnect, or remote
  PTY ownership after the SSH process exits; those belong to M5.
- tmux discovery, attachment, control mode, or tmux capability translation; those
  belong to M5.
- Password or keyboard-interactive credential storage in `strukt`.
- Replacing OpenSSH configuration, host-key policy, agent behavior, or key files.
- Supporting Windows as the remote host in the public-alpha M4 slice.
- Transparent local mirroring of an entire remote workspace.
- Offline editing or automatic conflict resolution after a disconnect.
- Remote port forwarding UI, Docker, Kubernetes, Git history visualization, or a
  general task scheduler.
- Automatically installing or upgrading a remote helper without explicit consent.
- Running arbitrary workspace commands, tasks, or language servers merely because
  a workspace was opened or restored.

## Product Model

### Connection identity

A connection target contains:

- a stable local identifier;
- the exact OpenSSH host alias selected by the user;
- a display label derived from that alias unless explicitly renamed;
- an optional recent remote workspace path;
- the last observed effective hostname, user, and port for display only;
- the last helper protocol and capability summary;
- connection health and last-used time.

The exact alias is the transport identity. `strukt` does not flatten it into a
hostname and hand-reimplement the rest of the SSH config. Effective values may be
obtained from `ssh -G` for preview and display, but OpenSSH remains authoritative at
connection time.

Connection records never contain private-key bytes, passphrases, agent tokens,
passwords, or copied `known_hosts` entries.

### Host discovery

The Connections view combines:

- explicit aliases the user adds;
- recent remote workspaces;
- best-effort literal aliases discovered from user SSH config files.

Discovery accepts only concrete `Host` tokens. Wildcards, negated patterns, and
catch-all entries affect connection behavior through OpenSSH but are not shown as
connectable aliases. `Include` discovery is bounded, cycle-safe, and best effort;
an unreadable or complex config never blocks an explicit alias.

Before a network connection, `ssh -G <alias>` validates that OpenSSH accepts the
target and supplies display metadata. A target is rejected if it contains NUL,
line breaks, starts with `-`, is empty, or exceeds the documented length bound.

### Workspace identity

A remote workspace identity is `(connection_id, normalized_remote_root)`. The
remote root is an absolute path or a helper-expanded `~/...` path. It is not
silently treated as a local filesystem path.

Recent remote workspace state persists locally in the normal user state store. No
`.strukt` metadata is written into the remote repository merely by opening it.

### Connection states

The normalized state machine is:

```text
Disconnected
  -> Connecting
  -> TerminalOnly
  -> NegotiatingHelper
  -> Ready

Connecting | TerminalOnly | NegotiatingHelper | Ready
  -> Stale
  -> Reconnecting
  -> TerminalOnly | NegotiatingHelper | Ready

any non-disconnected state
  -> Disconnecting
  -> Disconnected

any transition may produce Failed(reason, recovery), while preserving a direct
terminal action whenever OpenSSH itself remains usable.
```

`Stale` is visible and retains the last immutable snapshots. It never represents
fresh remote state. Reconnect uses capped exponential backoff with jitter and a
visible immediate-retry action. Explicit disconnect cancels retries.

## OpenSSH Adapter

### Executable discovery

The adapter resolves an executable without invoking a shell:

1. an explicit user setting;
2. `ssh` found through the process search path;
3. Windows system OpenSSH at the standard system location when applicable.

The resolved path and `ssh -V` result are diagnostic metadata, not persisted
credentials. An unavailable or unexecutable client produces an actionable local
error before a network attempt.

### Process modes

The adapter exposes separate typed operations:

- `resolve_config(alias)` using `ssh -G`;
- `probe(alias)` using a fixed no-output remote command and bounded deadline;
- `open_terminal(alias, cwd)` using the existing PTY/ConPTY abstraction and an
  interactive OpenSSH child;
- `open_helper(alias, executable)` using a non-interactive SSH stdio child;
- `install_helper(alias, artifact, metadata)` using a fixed bootstrap command and
  streamed bytes;
- `disconnect(operation_id)` through cooperative cancellation followed by bounded
  process termination.

Every operation has an identifier, deadline, stdout/stderr bounds, cancellation,
and a structured exit result. Standard error intended for OpenSSH diagnostics is
kept separate from framed helper stdout.

### Host verification and authentication

`strukt` does not set `StrictHostKeyChecking=no`, substitute a private known-hosts
file, or suppress changed-host warnings. Normal OpenSSH policy is preserved.

Interactive first connection, password, FIDO touch, and keyboard-interactive flows
run in the direct terminal surface. Helper and background operations are
non-interactive and fail with a recovery action if authentication requires a
prompt. This prevents an invisible process from hanging on credentials.

### Arguments and environment

Host aliases and fixed options are individual process arguments. User-provided
arbitrary extra SSH options are outside M4. The child inherits only the normal
platform environment needed by OpenSSH and the user's agent. Secrets printed by
OpenSSH are never copied into workspace persistence or helper logs.

## Remote Helper

### Packaging and installation

`strukt-remote` is a Rust workspace binary for Linux `x86_64` and `aarch64`. Release
packages include matching versioned helper artifacts and checksums. Development
builds may use an explicit matching helper artifact path.

The client determines the remote OS and architecture with a fixed OpenSSH command,
selects an exact supported artifact, verifies its local checksum, and displays the
host, remote install path, version transition, byte size, and checksum before
requesting consent. Approval is exact to that host, version, and artifact.

Installation streams bytes to a mode-`0600` temporary file under
`~/.local/share/strukt/bin`, verifies the remote checksum when a supported utility
is available, changes the final executable to mode `0700`, and atomically renames
it. The fixed bootstrap uses `umask 077`, follows no user-supplied path, requires no
elevation, and removes its temporary file on failure. The final path includes the
semantic helper version; an active compatible helper is never overwritten.

If no matching artifact is packaged, installation is unavailable with a precise
diagnostic; the terminal-only workspace remains usable.

### Protocol

The helper communicates over stdin/stdout only and exposes no socket or listening
port. The protocol uses a fixed magic preface followed by bounded length-prefixed
CBOR frames. Handshake fields include:

- protocol major and minor;
- helper semantic version and build target;
- random client nonce echoed by the helper;
- maximum request, response, stream chunk, and in-flight limits;
- supported capabilities;
- remote platform and canonical workspace root.

Major versions must match. Minor versions negotiate the intersection of known
capabilities. Unknown fields are ignored only where the schema marks them
extensible. Invalid magic, oversized frames, duplicate request identifiers,
out-of-order stream chunks, or post-cancellation data close the helper channel and
preserve terminal fallback.

Requests and responses carry stable operation identifiers. Streaming responses
carry monotonically increasing sequence numbers and explicit completion. Per-stream
credit bounds noisy output; the client grants more credit only after consuming the
previous window.

### Capabilities

M4 helper capabilities are independently advertised:

- filesystem metadata and paged directory listing;
- bounded file read and atomic conditional write;
- bounded workspace file enumeration for Quick Open;
- bounded text search;
- filesystem watch with resync markers;
- Git worktree summary;
- ephemeral process spawn, stdin, resize, cancel, and exit;
- language-server stdio transport;
- diagnostics and task-process events.

A missing capability disables only its commands and UI. The helper protocol does
not advertise persistent sessions in M4.

## Remote Files and Editing

### Root confinement

The helper opens the approved root once and resolves requests relative to it. Empty
segments, `.`, `..`, NUL, absolute child paths, platform prefixes, and normalized
escapes are rejected before filesystem access. Symlink behavior matches the local
workspace contract: a symlink entry is visible, but operations that follow it must
prove the canonical target remains within the workspace root.

The client treats remote paths as slash-separated opaque UTF-8 display paths for
the M4 Linux target. Invalid UTF-8 names remain representable as escaped bytes in
directory metadata but cannot be opened in the text editor.

### Listings, reads, and Quick Open

Directory listings are paged and sorted deterministically by the helper. Each entry
includes kind, size, modification time, and a stable revision token. The file tree
requests only expanded directories.

Quick Open consumes a bounded, cancellable file enumeration stream. It applies the
same ignored/hidden policy as local Quick Open where the remote helper can evaluate
it. Results are incrementally searchable and identify the remote host and root.

File reads are chunked and capped. Text decoding, binary detection, line endings,
dirty buffers, undo/redo, selections, and syntax behavior remain in the existing
editor. A read snapshot contains a revision derived from remote metadata and
content identity.

### Saves and conflicts

Saves send the expected remote revision. The helper writes a user-private sibling
temporary file, applies the existing file mode where safe, flushes, and atomically
replaces the target only if the expected revision still matches. A mismatch returns
a typed conflict; it never overwrites silently. The editor preserves the local
dirty buffer and offers reload or explicit overwrite with a newly confirmed
revision.

Disconnect during a save produces an unknown-outcome state. Reconnect must stat and
read the file before allowing another blind save.

### Watches and stale state

Watch events carry a generation and sequence. Overflow, helper restart, or missed
sequence marks the relevant tree stale and triggers a bounded rescan. The UI may
continue showing the last snapshot with a stale badge but cannot claim it is live.

## Remote Processes, Terminal, Language, and Tasks

### Direct terminal fallback

The direct terminal is an interactive OpenSSH child owned by the local app and
rendered through the existing terminal model. It is available before helper
installation and after helper failure. Its working directory is entered with a
fixed remote bootstrap that receives the normalized path as encoded stdin data;
the path is never interpolated into a shell command.

Closing or disconnecting the direct terminal ends that SSH process and its remote
shell. Persistence is intentionally deferred to M5.

### Helper-owned ephemeral processes

The helper can run an explicit executable and argument vector inside the workspace
root. It does not invoke a shell unless the user explicitly approves a shell task.
Output, input, resize, deadline, cancellation, and exit use bounded streams.

Workspace task commands use the existing exact-command approval model. Opening or
restoring a remote workspace never starts a task, language server, or arbitrary
process. A remembered approval is scoped to connection identity, workspace root,
executable, arguments, and relevant environment.

### Language intelligence

Existing language adapters remain language agnostic. Server discovery and launch
occur on the remote host through the helper; LSP bytes are transported unchanged
over a dedicated bounded stream. Paths are translated only at the workspace
boundary. Diagnostics, completion, hover, definition, restart, deadlines, and
stopped-only restoration retain the M2 behavior.

A language server is not downloaded or installed automatically. Missing servers
produce the same actionable disabled state as local workspaces.

### Search, Git, and diagnostics

Text search is cancellable, bounded, and streams matches with remote path and line
metadata. Git support in M4 is a read-only worktree summary suitable for status
chrome and changed-file decoration; staging, committing, and history visualization
remain later features.

Diagnostics originate from the remote language/task process but use the existing
Problems model and remain visibly scoped to the remote host.

## Workspace UI

### Connections activity

The existing Connections activity becomes a functional view containing:

- discovered and explicit host aliases;
- effective user/hostname/port preview where available;
- recent workspace roots;
- Open, Open Terminal, Reconnect, Disconnect, Forget, Install Helper, Repair
  Helper, and Copy Diagnostic actions gated by state and capability;
- compact connection, authentication, helper, and stale-state diagnostics.

The same stable command identifiers back keyboard and future command-palette
surfaces. Destructive or trust-changing actions require confirmation.

### Remote workspace boundary

When remote, the title/header, explorer heading and root, editor tabs or breadcrumbs,
terminal chrome, Problems labels, context surface, and status bar display the host
alias. Theme tokens represent connected, connecting, degraded/terminal-only,
stale, and failed states in both built-in themes without relying on color alone.

The file explorer stays pinned by default, toggles with the existing shortcut, and
never disappears merely because the Connections or Sessions view is active.

### Responsiveness

No SSH, helper, filesystem, search, process, or reconnect operation blocks the UI
thread. Immutable snapshots and bounded event queues cross into Iced. Each visible
long-running operation has cancellation and a stable status message. A noisy remote
process cannot starve file navigation or connection controls.

## Persistence

The local user store persists versioned remote connection records, recent roots,
workspace layout, editor presentation state, helper version metadata, and non-secret
diagnostics. It does not persist credentials, private keys, passphrases, agent
material, raw environment dumps, or helper request payloads.

Restoring the app creates a disconnected remote workspace snapshot. It does not
connect, authenticate, install a helper, start a task, start a language server, or
open a terminal until the user explicitly acts.

Forgetting a connection removes its local record and recent paths. It does not edit
SSH config, known hosts, agent state, or remote helper files. Remote helper removal
is a separate explicit operation.

## Security Requirements

- Preserve standard OpenSSH host-key verification; never disable it implicitly.
- Never parse or copy private-key contents.
- Treat host aliases, remote paths, filenames, Git text, task output, and helper
  frames as untrusted input.
- Use argument vectors, fixed remote bootstraps, framed stdin, strict lengths, and
  root-confined path resolution instead of command interpolation.
- Require exact consent before helper installation or upgrade.
- Verify local artifact checksum and remote installed bytes when supported.
- Install under the connected user with mode `0700` directories/executables and no
  elevation or public listener.
- Bound frames, queues, listings, reads, searches, process output, and reconnect
  attempts independently.
- Redact credential-related OpenSSH diagnostics from persisted logs.
- Scope command approvals to exact remote connection, root, executable, arguments,
  and environment.
- Keep remote and local operation identifiers separate to prevent cross-workspace
  cancellation or result routing.

## Failure and Recovery

- **OpenSSH absent:** fail locally with executable discovery guidance.
- **Config invalid:** show bounded `ssh -G` diagnostics and allow target editing.
- **Unknown or changed host key:** surface OpenSSH's failure; never auto-accept.
- **Authentication prompt required:** open or focus the direct terminal; background
  helper operations fail rather than hang.
- **Network loss:** cancel affected transfers, retain stale immutable snapshots,
  keep the UI responsive, and reconnect with bounded backoff.
- **Helper missing:** remain terminal-only and offer explicit installation.
- **Helper incompatible:** remain terminal-only and offer an exact versioned
  upgrade when a matching artifact exists.
- **Helper crash or invalid frame:** close the helper channel, mark helper-backed
  state stale, retain the direct terminal action, and offer restart/repair.
- **Listing/watch overflow:** mark the affected subtree stale and rescan.
- **Save unknown outcome:** re-read before another save.
- **Remote disk full or permission denied:** preserve the dirty editor buffer and
  return the typed remote error.
- **Process output overflow:** stop granting stream credit, retain bounded history,
  and keep unrelated operations responsive.
- **App restart:** restore disconnected presentation only; perform no remote side
  effect.

## Testing Strategy

### Unit and contract tests

- target validation, config alias discovery, include cycles, and `ssh -G` parsing;
- argument-vector construction with hostile aliases and paths;
- connection state transitions, retry caps, cancellation, and stale snapshots;
- protocol negotiation, capability intersection, bounds, sequences, cancellation,
  invalid frames, and forward-compatible fields;
- root confinement, symlink escapes, paging, file revisions, atomic writes,
  conflicts, invalid UTF-8 names, watch overflow, and resync;
- persistence migration and secret exclusion;
- command-approval scoping and UI capability gating.

### Integration fixtures

A repository-owned fake OpenSSH executable records exact argument vectors and can
launch the real helper over stdio. It provides deterministic config, authentication,
disconnect, stderr, partial-frame, latency, and process-exit scenarios on macOS,
Windows, and Linux without depending on a developer's SSH configuration.

The real `strukt-remote` helper runs against disposable directory trees for file,
Quick Open, edit/conflict, search, Git-summary, task, language-fixture, cancellation,
backpressure, and reconnect tests. No test writes workspace metadata.

### Real OpenSSH validation

The release gate includes a disposable Linux OpenSSH server or explicitly recorded
Linux host using generated non-production keys. It verifies standard config alias,
known-host enforcement, agent/key authentication, helper install without root,
remote workspace open, file edit, Quick Open, task, diagnostics, direct-terminal
fallback, disconnect, and reconnect. A real EC2 walkthrough may satisfy the same
gate when the host and credentials are available, but secrets and host details are
not committed.

The full matrix runs on macOS 14, Windows Server 2022, and Ubuntu 24.04. Native
macOS and Windows walkthroughs verify boundary labels, keyboard access, responsive
stale state, helper consent, and terminal fallback.

## Acceptance Criteria

1. A concrete OpenSSH alias can be added or discovered and previewed with the
   platform OpenSSH client on macOS, Windows, and Linux.
2. Connecting preserves normal config, key, agent, known-host, and `ProxyJump`
   behavior and never disables host verification.
3. A Linux SSH host can be opened as a remote workspace with an explicit root.
4. The host alias is visible in the header, explorer, editor context, terminal,
   diagnostics, and status area.
5. A direct SSH terminal remains usable without the helper and after helper failure.
6. Installing or upgrading the helper requires exact consent, uses no root access,
   exposes no public port, and validates the selected artifact.
7. The real helper supports remote tree browsing, Quick Open, read, conditional
   atomic save, text search, Git summary, approved ephemeral tasks, diagnostics,
   and language-server transport within the approved root.
8. Disconnect leaves the local UI responsive and marks retained state stale;
   reconnect resynchronizes generations and sequences without routing old results
   into the new connection.
9. Opening or restoring a workspace starts no connection, helper install, terminal,
   task, language server, or arbitrary remote command implicitly.
10. Credentials and private keys never enter workspace or connection persistence,
    and opening a remote workspace creates no `.strukt` repository metadata.
11. Hostile aliases, paths, helper frames, noisy output, conflicts, cancellation,
    helper crashes, and protocol mismatch have direct automated regression coverage.
12. Strict format, lint, all-target tests, deterministic remote smoke, and the
    exact final head pass on macOS, Ubuntu, and Windows.

## M5 Boundary

M5 may reuse the OpenSSH adapter, helper installation, framed protocol, connection
identity, capability negotiation, path confinement, bounded streams, and stale
reconnect model. M5 owns remote PTY persistence, session/window/pane inventory,
missed output after disconnect, local app restart reattachment, concurrent remote
session fairness, reboot restoration, and tmux providers.

M4 must not fake persistence by keeping an invisible local SSH child after the user
disconnects or by relabeling an ephemeral terminal as a session.
