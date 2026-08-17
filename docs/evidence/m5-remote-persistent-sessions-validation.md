# M5 Remote Persistent Sessions Validation

- Date: 2026-08-17
- Candidate implementation head: `1128643`
- Hosted implementation run: [32051173499](https://github.com/js503/strukt/actions/runs/32051173499) (in progress)
- Issue: [#15](https://github.com/js503/strukt/issues/15)
- Pull request: [#16](https://github.com/js503/strukt/pull/16)
- Spec: [`../specs/0009-m5-remote-persistent-sessions.md`](../specs/0009-m5-remote-persistent-sessions.md)
- Plan: [`../plans/0009-m5-remote-persistent-sessions.md`](../plans/0009-m5-remote-persistent-sessions.md)

## Outcome

M5 adds first-class persistent sessions to an SSH workspace. The native provider
reuses the M3 session hierarchy behind the versioned M4 helper, starts its detached
per-user service only for explicit session intent, and preserves running PTYs,
layout, bounded history, and service identity across SSH helper replacement. A
capability-limited tmux provider discovers and attaches existing sessions, sends
literal input, resizes panes, captures bounded output, and detaches without
terminating the tmux server.

The Sessions surface now carries the durable connection identity, remote host,
provider, availability, health, and stale state. Native and tmux actions are
capability-gated, provider changes wait for the single in-flight request lane, and
an SSH reconnect cannot accidentally rebind a session projection from another
connection record. Explorer and the M4 direct-terminal fallback remain available.

## Local release gate

The implementation head passed the complete macOS release gate:

```text
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline --quiet
cargo build -p strukt-app -p strukt-remote -p strukt-session --bins --locked --offline
bash scripts/m5-remote-sessions-smoke.sh
```

All commands completed successfully. The app target reported 136 passing tests,
every workspace test target passed with zero failures, the required application,
remote-helper, session-service, fixture, and smoke binaries built, and the M5
smoke printed:

```text
strukt M5 remote session smoke: multiple native sessions, SSH detach, service identity, output, layout, and reconnect passed
```

The smoke creates two isolated native remote sessions, adds another window and
split pane, starts real fixture PTYs, writes and observes independent markers,
replaces the SSH helper, verifies the same service instance and running panes,
restores the hierarchy and retained output, cleans up explicitly, and confirms no
`.strukt` workspace metadata was written.

## Real OpenSSH and native service

Ubuntu CI starts a disposable localhost-only OpenSSH server with generated
non-production keys, strict known-host checking, password authentication disabled,
and a private temporary workspace. The ignored integration test is made mandatory
in that job and runs the packaged helper and native session-service mode through
the real `ssh` executable.

The test creates and starts a real remote PTY, records the native service instance,
observes a marker in the pane snapshot, disconnects the first SSH helper, connects
a new helper generation, and verifies both the same service instance and retained
marker output before explicit cleanup. This proves the process/service identity
boundary across a real SSH transport. The deterministic cross-platform smoke
separately proves multiple sessions, windows, and split panes.

No live EC2 account or production credential was used. The validated standard
OpenSSH alias path is vendor-independent and is the same path an EC2 connection
uses; a human EC2 onboarding walkthrough remains an Alpha release gate.

## Real and deterministic tmux

The tmux suite contains deterministic cross-platform tests for fixed argument
vectors, hostile names as data, ID validation, bounded discovery and capture,
literal hexadecimal input, numeric resize, detach, malformed control records,
provider capability reduction, and output-process termination at the bound.

Ubuntu CI first requires `tmux` to exist and prints its version, so the real-tmux
test cannot silently skip. The test creates a uniquely named private tmux server,
discovers its session and pane, attaches through the shared provider protocol,
sends and observes marker output, detaches, proves native-only operations are
rejected, and removes only the exact private server it created.

## Hosted platform matrix

GitHub Actions run
[32051173499](https://github.com/js503/strukt/actions/runs/32051173499) is the
candidate implementation-head validation on macOS 14, Ubuntu 24.04, and Windows
Server 2022. Every
job runs formatting, strict Clippy, the full workspace test suite, required binary
builds, and the milestone smoke chain. Ubuntu additionally makes real tmux and the
disposable real-OpenSSH/native-service path hard prerequisites.

The final documentation head is required to pass the same three jobs before PR
#16 is marked ready and merged; that exact-head result is recorded in the pull
request because a commit cannot contain its own future Actions run identifier.

## Full-slice review

Overall review risk after fixes is moderate-low for the M5 scope. The review
covered the entire M3/M4 composition boundary, helper protocol, service launch,
tmux adapter, desktop reducer and UI projection, persistence, tests, smoke scripts,
and CI workflow. It found and resolved these material issues:

- remote reconnect identity was keyed by a display alias instead of the durable
  connection record ID;
- provider switching could race an in-flight completion into a newly selected
  backend;
- session reconnect attempts did not require the live SSH runtime, and SSH
  reconnect could overlap a provider operation;
- catalog replacement retained snapshots for panes that no longer existed;
- the detached Unix session-service launch did not explicitly enter a new process
  group;
- real tmux could silently skip when absent from the Ubuntu runner;
- deterministic tmux executable fixtures used a Unix-only absolute path on
  Windows;
- an SSH helper descendant could retain the diagnostic pipe on Windows, causing
  disconnect to wait forever for the diagnostic reader thread;
- the approved spec overstated a delta/event-queue design that M5 does not ship.

Focused regressions cover durable connection identity, the single request lane,
removed-pane pruning, platform-native executable validation, bounded diagnostic
reader shutdown, explicit start, provider correlation, process-output bounds, and
packaged service mode. Hosted Windows runs exposed both the fixture-path and
descendant-held-pipe issues; each corrected focused suite and the local M5 smoke
passed before its replacement matrix ran.

No unresolved critical or important implementation finding remains. The known
validation gaps below belong to the explicit Alpha gate, not to a hidden M5 claim.

## Security and boundedness review

Review confirmed that user-controlled aliases, roots, tmux names, identifiers, and
terminal bytes are never shell-interpolated. The helper and tmux adapter use
validated fixed argument vectors; tmux input is hexadecimal data. Session payloads
are capped at 1 MiB, tmux command output and control records are bounded, M3
scrollback/service limits remain authoritative, and the app permits only one
provider request in flight.

The service is launched only for explicit create, attach, or restart intent, uses
private per-user IPC and persistence, detaches from SSH stdio and its Unix process
group, and never opens a public port or requires root. The rendezvous secret stays
on the remote host. Persisted desktop state contains provider preference and opaque
selection IDs, not credentials, terminal bytes, tmux environment, raw protocol
payloads, or rendezvous material.

## Accepted Alpha limitations

- M5 performs a bounded full pane resync after reconnect. Pane cursor fields are
  retained so ordered delta replay can be added later without changing identity;
  M5 does not claim delta streaming.
- tmux uses bounded fixed-argument request/response operations. It validates the
  required control-record subset for a future streaming transport but does not
  keep a long-lived control client in M5.
- The native provider enforces its authenticated single-writer session-service
  lease. The compatibility tmux provider follows tmux's existing client model and
  adds no cross-helper writer lease.
- A live EC2 walkthrough, signed installers, human Windows and macOS visual and
  keyboard walkthroughs, IME/accessibility certification, packaging, and release
  documentation remain Public Alpha gates.
- Visual refinement may continue during Alpha work; it does not weaken the tested
  host, provider, stale-state, capability, or Explorer affordances delivered here.

These limitations do not permit implicit remote execution, provider fallback,
credential persistence, public listeners, root privilege, shell interpolation,
unbounded output, or a false claim that the Public Alpha itself has shipped.
