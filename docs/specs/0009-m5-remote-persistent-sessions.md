# M5 Remote Persistent Sessions

- Status: Approved for implementation
- Date: 2026-08-09
- Governing spec: [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)
- Depends on: [`0007-m3-local-persistent-sessions.md`](0007-m3-local-persistent-sessions.md) and [`0008-m4-ssh-remote-workspace.md`](0008-m4-ssh-remote-workspace.md)
- Roadmap: [`../roadmap.md`](../roadmap.md)
- Interaction reference: [`../mockups/workspace-shell/remote-multiplexer.html`](../mockups/workspace-shell/remote-multiplexer.html)

## Summary

M5 makes persistent terminal sessions a first-class part of an SSH workspace. A
developer can run multiple named sessions, windows, and split panes on one remote
Linux host; disconnect SSH or quit the local application; and reconnect to the
same running processes, layout, and retained output. The native provider is the
recommended path. Existing tmux sessions are also discoverable and attachable
through an explicit provider with accurately reduced capabilities.

The M4 helper remains a short-lived SSH stdio process. For native sessions it
proxies the existing M3 session protocol to a detached, per-user session-service
mode hosted by the installed `strukt-remote` runtime. The service owns PTYs and persistence and
listens only on private local IPC. Its rendezvous secret never crosses SSH to the
desktop. Opening or restoring a workspace starts no remote process; service start
is limited to an explicit create, attach, or restart action.

For tmux, the helper invokes the installed `tmux` executable with fixed argument
vectors for discovery, literal input, resize, and bounded capture. It never constructs a
shell command from a session name or pane identifier. Native and tmux sessions
share the user-facing session/window/pane hierarchy, but the UI gates every
action from provider-advertised capabilities and never silently changes provider.

## Goals

- Keep multiple independent session hierarchies running on one SSH host through
  SSH interruption and local application restart.
- Restore retained output without duplicating or misordering bytes after
  reconnect.
- Preserve M3 naming, layout, lifecycle, persistence, bounds, and explicit restart
  policy for the native remote provider.
- Discover and interactively attach existing tmux sessions without attempting
  complete tmux configuration or command parity.
- Make host, provider, connection health, stale state, and recovery actions visible
  in the existing Sessions surface.
- Keep Explorer immediately accessible while sessions are active.
- Preserve terminal-only SSH and the full M4 workspace when persistent-session
  support is missing, incompatible, or unhealthy.
- Keep macOS and Windows desktop clients behaviorally equivalent while targeting
  a Linux remote host for the public alpha.

## Non-goals

- Windows remote hosts for the public alpha.
- Multiple simultaneous controlling clients for the native provider. One native
  helper/client holds the service writer lease; additional native clients receive
  a precise busy response. The compatibility tmux provider follows tmux's existing
  client model and does not add a separate cross-helper lease in M5.
- Collaboration, shared cursors, session sharing, or multi-user authorization.
- Full tmux command, configuration, plugin, status-line, hook, copy-mode, or key
  binding parity.
- Creating, renaming, splitting, duplicating, terminating, or restarting tmux
  sessions from `strukt` in M5. The tmux provider discovers, attaches, accepts
  terminal input and resize, captures bounded history, and detaches.
- Automatically restarting arbitrary commands after remote reboot.
- Automatically installing tmux or enabling tmux when it is absent.
- Public network listeners, root privileges, credential storage, or repository
  metadata written merely by opening a workspace.
- AI, collaboration, plugin, MCP, container, or Kubernetes features.

## Architecture

```text
strukt-app
  -> platform OpenSSH stdio connection (M4)
     -> versioned strukt-remote helper (per SSH connection)
        -> native adapter
           -> authenticated private local IPC
              -> detached per-user native session-service mode
                 -> remote PTYs + private persistent store
        -> tmux adapter
           -> fixed argv tmux adapter
              -> existing per-user tmux server
```

The desktop owns presentation and immutable projections. `strukt-remote` owns SSH
transport, provider discovery, helper proxying, tmux translation, bounds, and
connection generations. `strukt-session` remains the source of truth for provider
identities, session protocol envelopes, catalogs, snapshots, lifecycle, and
capabilities. Provider-specific code does not enter the editor, file explorer,
terminal renderer, or shell-state crates.

### Provider identity

A remote provider identity is:

```text
(connection_id, remote_user_identity, provider_kind)
```

It is host-scoped, not workspace-root-scoped. Two remote workspaces on the same
effective host and user observe the same native and tmux provider catalogs. The
desktop does not infer this identity from a display hostname alone; it binds the
provider to the M4 connection record and the helper's effective user/host
projection. Provider-scoped opaque session, window, and pane identifiers are never
mixed across native, tmux, local, or connection generations.

### Provider capabilities

The native remote provider advertises the same lifecycle, window, pane, input,
resize, snapshot, and structured-history capabilities as native local sessions.
Its provider kind is `native-remote` and the UI labels it `strukt native`.

The M5 tmux provider advertises:

- catalog and discovery;
- attach and detach;
- terminal input and resize;
- bounded screen/history snapshots.

It does not advertise native create, rename, duplicate, terminate, restart,
structured command history, or layout mutation. Unsupported actions remain hidden
or disabled with an explanation. No provider failure causes an automatic fallback
to a different provider.

## Remote Helper Protocol Extension

### Negotiation

M5 adds independent `sessions` and `tmux` helper capabilities. `sessions` is
advertised only when the helper understands both the session tunnel schema and its
built-in native session-service mode. `tmux` is advertised when an executable path
is detected; the first explicit tmux provider operation validates its output and
identifiers before presenting a catalog.

The existing major-version rule remains strict. Minor versions negotiate known
capabilities. A sessiond protocol incompatibility disables only native remote
sessions; a tmux incompatibility disables only tmux. Files, editing, ephemeral
terminal, Git, tasks, diagnostics, and language transport remain available.

### Session tunnel

The helper protocol adds typed operations for provider discovery, native-service
status/start, provider attach/detach, session request exchange, and bounded tmux
events. A native exchange carries a CBOR-encoded M3 `RequestEnvelope` or
`ResponseEnvelope` inside a length-bounded session payload. The outer request is
correlated by the M4 operation ID; the inner request is correlated by the session
request ID and expected catalog revision.

The maximum encoded session payload is 1 MiB. Decoding occurs only after the outer
frame and capability are validated. Invalid inner framing, unexpected response
identity, oversized data, or post-cancellation events close that provider channel
without closing the M4 workspace channel when safe recovery remains possible.

The remote rendezvous secret is loaded by the helper from the private remote
application-data directory. It authenticates private local IPC and is never placed
in a helper response, desktop state, log, or workspace file.

### Generations and reconnect

Every remote session projection is bound to both:

- the M4 SSH connection generation; and
- the provider service instance identifier.

Responses or events from an older generation are ignored. If the service instance
changes, the client discards prior writer ownership and reattaches before sending
input.

For every observed pane the client retains the last accepted screen revision and
output sequence. Reattach supplies those cursors. The public-alpha implementation
uses an explicit bounded full resync for every retained pane after reconnect;
replacement atomically discards the stale pane projection. The cursor contract
keeps exact ordered delta replay additive later. Gaps, duplicate responses, and
service-instance changes force full replacement rather than speculative merge.

The last immutable catalog and pane snapshots remain visible while the connection
is stale. They are clearly marked stale, accept no input, and expose reconnect and
terminal-fallback actions.

## Native Remote Service

### Packaging and lifecycle

Each supported Linux release package contains one `strukt-remote` runtime artifact
with the helper and native session-service mode, version metadata, and a checksum.
Installation and repair reuse M4's exact host/version/artifact consent and private
atomic install rules. No binary is downloaded by the remote host, and there is no
second artifact whose version or checksum can drift independently.

The helper starts the exact installed service executable only after an explicit
create, attach, or restart intent. It passes a fixed private application-data path,
sets stdin/stdout/stderr to null, detaches it from the SSH channel/process group,
and waits for a valid private rendezvous record with capped backoff. It never uses
a shell, user-supplied executable, workspace startup hook, root privilege, public
port, or repository-local state.

Catalog, provider-status, workspace-open, and application-restore operations do
not start the service. When no service is running, persisted definitions may be
projected as stopped by a bounded read-only store inspection; attaching or
restarting one remains explicit.

### Persistence and reboot

The remote service reuses M3's private atomic store and size/count bounds. SSH
disconnect, helper exit, and desktop exit do not detach or terminate the service.
An explicit provider detach releases the controlling client but does not terminate
session processes.

A remote reboot stops every PTY process. On the next explicit service start, saved
session/window/pane definitions, layout, names, working directories, and bounded
history load with stopped lifecycle state. No arbitrary command is restarted until
the user explicitly restarts that session or pane under the existing M3 policy.

### Fairness and bounds

M3 per-pane scrollback and service limits remain authoritative. The
request-response proxy adds:

- at most one in-flight native exchange per attached client;
- at most 1 MiB per inner frame;
- no asynchronous provider-event queue in M5;
- bounded reconnect attempts and fixed capped backoff;
- round-robin pane/event draining so one noisy process cannot starve another;
- cancellation and stale-generation rejection before projection updates.

Crossing a bound produces a typed overflow/resync condition. It never permits
unbounded allocation or blocks the app event loop.

## tmux Provider

### Discovery

The helper resolves `tmux` from a trusted configured path or the normal remote
process search path, validates a supported version, and invokes it directly. It
uses fixed `list-sessions`, `list-windows`, and `list-panes` argument vectors with
a repository-owned machine format. User-controlled names are data returned by
tmux, not shell input.

Raw tmux identifiers are validated and mapped to provider-scoped opaque IDs. The
adapter tolerates sessions disappearing between discovery and attach and returns a
typed not-found result. Malformed, oversized, or ambiguous output disables that
refresh and retains the prior stale snapshot.

### Interactive attachment

Provider attach discovers and projects the existing tmux hierarchy through stable
opaque IDs while the topology is unchanged. Selecting a tmux session, window, or
pane remains presentation-local rather than mutating tmux focus. Literal terminal
input uses fixed-argv hexadecimal `send-keys`; resize accepts only validated numeric
dimensions; initial attach and resync use bounded `capture-pane` output. The
adapter also validates the documented control-mode records needed by a future
streaming transport, but M5 does not keep a long-lived control client.

tmux history is exposed as a screen snapshot rather than native structured command
history. Detach releases only the strukt provider projection and leaves the tmux
server and sessions running.

## User Experience

The existing Sessions surface gains a remote host header and provider switcher.
`strukt native` is first and marked recommended. `tmux` appears when available and
is described as compatibility for existing sessions. Session rows and the active
terminal header show host and provider badges. Local and remote providers cannot
be mistaken for one another.

The Explorer activity item and keyboard shortcut remain available while a remote
session is active. Switching files, sessions, windows, and panes uses the existing
Focus + Context shell rather than replacing the workspace with a terminal-only
screen.

Disconnected sessions keep the last snapshot with a visible stale banner. Input,
resize, and destructive actions are disabled until successful reattach. Recovery
offers retry, repair native service when applicable, switch provider explicitly,
or open the existing direct SSH terminal.

All provider actions are keyboard reachable, theme-token based, and exposed
through shell commands so workflows remain scriptable and reproducible.

## Security and Privacy

- No public listening port is opened by the helper, native service, or tmux
  adapter.
- Remote processes run as the authenticated SSH user and never require root.
- The combined helper and native-service runtime requires exact checksum-bound consent.
- Rendezvous files, stores, and sockets use private per-user permissions.
- SSH keys, passwords, agent material, rendezvous secrets, and tmux environment
  values are never persisted by the desktop.
- User aliases, roots, tmux names, and identifiers are never interpolated into a
  shell command.
- Workspace open and restore execute no session, tmux, or service side effect.
- Logs redact remote command content and terminal bytes by default.
- Telemetry and crash upload remain absent unless separately designed as opt-in.

## Failure and Recovery Matrix

| Failure | Visible result | Recovery |
|---|---|---|
| SSH interruption | Last snapshots marked stale; input disabled | Retry with cursor-aware reattach or open direct terminal |
| Native service absent | Provider shown stopped | Explicit create/attach/restart starts it |
| Native service incompatible | Native provider unavailable; M4 workspace remains ready | Exact artifact repair with consent |
| Native service dies | Provider stale/stopped; sessions reflect actual process loss | Explicit restart; never implicit command restart |
| Output cursor expired | Pane requests full bounded resync | Atomic snapshot replacement |
| Writer already attached | Readable busy state; no input lease | Detach the other controller or retry later |
| tmux absent | tmux provider unavailable | Install externally or use native provider |
| tmux session disappears | Attached surface becomes stopped/not found | Refresh catalog and choose another session |
| tmux output malformed/oversized | Prior projection marked stale | Detach/retry; native provider unaffected |
| Helper protocol incompatible | Persistent providers unavailable | Repair helper or retain terminal-only/full M4 workspace |

## Verification Strategy

### Deterministic automated coverage

- Provider capability matrices and provider-scoped identity isolation.
- Helper protocol negotiation, tunnel framing, bounds, cancellation, and stale
  generation rejection.
- Native remote service explicit-start policy, private rendezvous proxying,
  detach/reconnect, delta replay, full resync, reboot-stopped restoration, and
  writer lease behavior.
- Concurrent noisy sessions proving bounded queues and fair progress.
- Fake tmux discovery/control streams covering valid, hostile, partial, malformed,
  disappearing, oversized, input, resize, resync, and detach behavior.
- App projections and UI tests for host/provider badges, capability-gated actions,
  Explorer accessibility, stale state, reconnect, and terminal fallback.
- Cross-platform desktop tests on macOS, Windows, and Linux using fake SSH/provider
  fixtures.

### Integration coverage

- A disposable local OpenSSH Linux environment runs the packaged helper and
  detached service, creates multiple real PTYs, disconnects SSH, reconnects, and
  proves process identity/output/layout continuity.
- A real tmux executable, when present in the Linux CI environment, is used to
  create, discover, attach, exchange marker output, detach, and prove the tmux
  session remains alive. The deterministic fake remains mandatory everywhere.
- Protocol upgrade tests prove compatible minor negotiation and isolated fallback
  for an incompatible helper or session service.
- Earlier M1-M4 smoke and full workspace gates remain green.

### Human alpha gates

Before public alpha, macOS and Windows walkthroughs verify rendering, keyboard-only
operation, themes, accessibility semantics, IME input, stale/reconnect messaging,
Explorer access, provider switching, and an EC2-backed Linux host. Automated
evidence is not represented as human verification.

## Acceptance Criteria

1. A user can explicitly create and attach multiple native sessions on one remote
   Linux host, each with independent windows, split panes, names, layout, working
   directories, lifecycle, and bounded history.
2. Native session processes and PTY identity survive SSH helper disconnect and
   local application restart.
3. Reconnect applies exact ordered deltas or an explicit full resync without
   duplicate, missing, or cross-generation output.
4. One noisy session cannot starve another or make file/editor interactions
   unresponsive.
5. Remote reboot restores definitions, layouts, names, and retained history as
   stopped and never restarts arbitrary commands implicitly.
6. Existing tmux sessions are discovered and can be interactively attached,
   resized, given input, resynced, and detached without terminating them.
7. Native and tmux capability differences are visible and enforced; no silent
   provider fallback occurs.
8. Host/provider/stale boundaries are visible, and Explorer remains immediately
   accessible during session work.
9. Opening or restoring a workspace starts no helper service, session service,
   tmux client, PTY, task, or command.
10. No public port, root privilege, private credential persistence, shell
    interpolation, or repository metadata is introduced.
11. Missing or incompatible session support leaves the M4 workspace and direct SSH
    terminal usable.
12. The exact final head passes strict formatting, lint, full workspace tests,
    deterministic M5 smoke, disposable real-SSH/native-service integration, real
    tmux integration where available, and macOS/Ubuntu/Windows hosted checks.
