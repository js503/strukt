# M5 Remote Persistent Sessions Implementation Plan

> Execute every behavior change test-first. Preserve the M3 session domain and M4
> SSH workspace as independent foundations; M5 composes them through provider and
> transport adapters.

**Goal:** Deliver multiple native persistent session hierarchies on one remote
Linux host, exact reconnect recovery, existing tmux discovery/attachment, cohesive
host/provider-aware UI, and cross-platform release evidence without weakening M4
fallback or security boundaries.

**Architecture:** Extend the M4 helper protocol with bounded persistent-provider
operations. Proxy native exchanges to a detached per-user `strukt-sessiond` over
its existing authenticated local IPC. Add a separate fixed-argv tmux adapter. Add
remote provider backends to the M3 `SessionClient` and project them through the
existing Sessions surface with explicit capabilities and connection generations.

**Tech stack:** Rust 2024, existing CBOR protocols and BLAKE3 identities, platform
OpenSSH, private local IPC, `portable-pty`, tmux control mode, Iced, serde
persistence, deterministic fake SSH/sessiond/tmux fixtures, and strict workspace
CI.

**Spec:** [`../specs/0009-m5-remote-persistent-sessions.md`](../specs/0009-m5-remote-persistent-sessions.md)

---

## Execution constraints

- [ ] Follow red-green-refactor for every implementation task.
- [ ] Keep workspace open/restore side-effect free.
- [ ] Never construct shell commands from host aliases, paths, tmux names, IDs, or
      terminal input.
- [ ] Start the native service only for explicit create, attach, or restart intent.
- [ ] Keep rendezvous authentication secret on the remote host.
- [ ] Keep frames, queues, history, retries, control records, and diagnostics
      bounded.
- [ ] Gate every provider action by advertised capabilities.
- [ ] Preserve the M4 workspace and direct terminal when persistent providers fail.
- [ ] Never imply that automated Windows/macOS UI checks replace human alpha
      walkthroughs.
- [ ] Do not add collaboration, tmux parity, Windows remote hosts, AI, plugins, or
      post-alpha features.

## Task 1: Extend provider and reconnect contracts

**Files:**

- Modify: `crates/strukt-session/src/provider.rs`
- Modify: `crates/strukt-session/src/protocol.rs`
- Modify: `crates/strukt-session/src/snapshot.rs`
- Modify: `crates/strukt-session/src/client.rs`
- Modify: `crates/strukt-session/src/lib.rs`
- Modify: `crates/strukt-session/tests/provider.rs`
- Modify: `crates/strukt-session/tests/protocol.rs`
- Modify: `crates/strukt-session/tests/client.rs`

- [ ] **Step 1: Write failing provider-matrix tests**

Cover native-local/native-remote parity, the exact M5 tmux capability subset,
provider-scoped identity, display labels, and rejection of unsupported actions.

- [ ] **Step 2: Implement explicit provider capabilities**

Add named constructors for native remote and tmux interoperability. Keep provider
kind data in every catalog/snapshot projection.

- [ ] **Step 3: Write failing reconnect-cursor tests**

Cover service instance changes, pane output cursors, exact-next delta application,
duplicate suppression, sequence gaps, expired history, full resync, stale client
health, and old-generation rejection.

- [ ] **Step 4: Implement bounded reconnect types and reducers**

Add per-pane cursors, delta/resync response variants, checked snapshot replacement,
and client generation guards without transport dependencies.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-session --all-targets --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
git add crates/strukt-session
git commit -m "feat: add remote session reconnect contracts"
```

## Task 2: Extend and bound the remote helper protocol

**Files:**

- Modify: `crates/strukt-remote/Cargo.toml`
- Modify: `crates/strukt-remote/src/protocol.rs`
- Modify: `crates/strukt-remote/src/helper.rs`
- Modify: `crates/strukt-remote/src/lib.rs`
- Modify: `crates/strukt-remote/tests/protocol.rs`
- Modify: `crates/strukt-remote/tests/helper.rs`

- [ ] **Step 1: Write failing negotiation and framing tests**

Cover independent sessions/tmux capabilities, compatible minor intersection,
incompatible session payload fallback, 1 MiB inner limits, outer/inner request
correlation, cancellation, response mismatch, invalid CBOR, and post-cancel data.

- [ ] **Step 2: Implement typed provider operations**

Add provider discovery/status/start, attach/detach, bounded native exchange, and
bounded tmux event responses. Validate outer framing and capability before decoding
inner session envelopes.

- [ ] **Step 3: Write failing helper isolation tests**

Prove a native or tmux provider error remains isolated from files, Git, processes,
language transport, and direct terminal fallback.

- [ ] **Step 4: Implement helper dispatch boundaries**

Add injected native/tmux managers with separate locks and typed unsupported,
unavailable, busy, stale, and resync errors.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-remote --test protocol --test helper --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add Cargo.lock crates/strukt-remote
git commit -m "feat: extend the remote provider protocol"
```

## Task 3: Package and manage the detached native service

**Files:**

- Modify: `crates/strukt-remote/src/artifact.rs`
- Create: `crates/strukt-remote/src/native_session.rs`
- Modify: `crates/strukt-remote/src/helper.rs`
- Modify: `crates/strukt-session/src/bin/strukt-sessiond.rs`
- Create: `crates/strukt-remote/tests/native_session.rs`
- Modify: `crates/strukt-remote/tests/artifact.rs`
- Modify: `crates/strukt-remote/tests/helper.rs`

- [ ] **Step 1: Write failing artifact and launch tests**

Cover matching architecture/version/checksum metadata, exact consent, private
atomic install, absent/incompatible service, fixed executable/arguments, detached
stdio/process group, no shell, no root, and no start for status/catalog/open.

- [ ] **Step 2: Implement sessiond artifact selection and explicit launch**

Reuse M4 install trust and permissions. Add an injected launcher that starts only
for explicit create/attach/restart and waits for a private valid rendezvous record
with capped backoff.

- [ ] **Step 3: Write failing authenticated proxy tests**

Cover secret retention on remote, private endpoint discovery, writer lease, helper
disconnect without service termination, reconnect to the same instance, service
instance replacement, exchange correlation, frame bounds, and service death.

- [ ] **Step 4: Implement the native provider proxy**

Connect through the existing `strukt-session` client/backend contracts and relay
only validated session envelopes. Return stopped/incompatible/busy state without
affecting M4 helper availability.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-remote --test artifact --test native_session --test helper --locked --offline
cargo test -p strukt-session --bin strukt-sessiond --locked --offline
cargo clippy -p strukt-remote -p strukt-session --all-targets --locked --offline -- -D warnings
git add Cargo.lock crates/strukt-remote crates/strukt-session
git commit -m "feat: proxy detached native remote sessions"
```

## Task 4: Implement exact reconnect and fair output recovery

**Files:**

- Modify: `crates/strukt-session/src/service.rs`
- Modify: `crates/strukt-session/src/screen.rs`
- Modify: `crates/strukt-session/src/protocol.rs`
- Modify: `crates/strukt-session/src/bin/strukt-sessiond.rs`
- Modify: `crates/strukt-session/tests/service.rs`
- Modify: `crates/strukt-session/tests/screen.rs`
- Create: `crates/strukt-session/tests/reconnect.rs`

- [ ] **Step 1: Write failing delta/resync service tests**

Cover cursors for multiple panes, exact retained replay, expired cursor full
snapshot, instance mismatch, duplicate/gap rejection, stopped panes, and atomic
catalog/screen revision snapshots.

- [ ] **Step 2: Implement retained output cursors**

Record bounded per-pane output revisions and provide a single checked reducer for
delta or full-snapshot reconnect responses.

- [ ] **Step 3: Write failing fairness and overflow tests**

Drive multiple real or fixture PTYs with one noisy producer; prove round-robin
progress, 4 MiB proxy and 1,024-event caps, typed overflow, resync recovery, and
responsive catalog/input work for quiet sessions.

- [ ] **Step 4: Implement fair draining and bounded resync**

Use capped per-pane work per tick and explicit resync markers. Never hold a global
service lock while blocking on PTY or transport I/O.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-session --test reconnect --test service --test screen --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
git add crates/strukt-session
git commit -m "feat: recover remote session output exactly"
```

## Task 5: Add the tmux interoperability provider

**Files:**

- Create: `crates/strukt-remote/src/tmux.rs`
- Create: `crates/strukt-remote/tests/tmux.rs`
- Create: `fixtures/tmux/fake-tmux.rs`
- Create: `fixtures/tmux/control/attach.txt`
- Create: `fixtures/tmux/control/malformed.txt`
- Modify: `crates/strukt-remote/src/helper.rs`
- Modify: `crates/strukt-remote/src/lib.rs`

- [ ] **Step 1: Write failing discovery tests**

Cover executable/version detection, fixed list argument vectors, session/window/pane
mapping, hostile names as data, stable opaque IDs, duplicates, disappearing
sessions, malformed/partial/oversized output, deterministic sorting, and absence.

- [ ] **Step 2: Implement bounded tmux discovery**

Invoke tmux directly with repository-owned formats, validate raw IDs, map them into
provider scope, and retain the last snapshot as stale on recoverable refresh error.

- [ ] **Step 3: Write failing control-mode tests**

Cover exact attach argv, output/layout/focus records, escaped payloads, input,
resize, capture/resync, detach, unknown records, required-record corruption,
bounded lines/queues, cancellation, and a session disappearing mid-attach.

- [ ] **Step 4: Implement the capability-limited tmux adapter**

Parse the required control-mode subset, expose snapshots and input/resize/detach,
and reject native-only lifecycle/layout actions before process invocation.

- [ ] **Step 5: Add optional real-tmux integration**

When `tmux` is present, create a private test server/socket, discover and attach a
marker session, exchange output/input, detach, prove the session remains alive,
and clean it up by exact test identifier. Keep fake-tmux mandatory on every OS.

- [ ] **Step 6: Verify and commit**

```bash
cargo test -p strukt-remote --test tmux --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add crates/strukt-remote fixtures/tmux
git commit -m "feat: add tmux session interoperability"
```

## Task 6: Connect remote providers to app session state

**Files:**

- Modify: `crates/strukt-app/src/session.rs`
- Modify: `crates/strukt-app/src/remote.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/message.rs`
- Modify: `crates/strukt-app/src/persistence.rs`
- Modify: `crates/strukt-app/tests/session_ui.rs`
- Modify: `crates/strukt-app/tests/remote_ui.rs`
- Modify: `crates/strukt-persistence/src/lib.rs`
- Modify: `crates/strukt-persistence/tests/state.rs`

- [ ] **Step 1: Write failing backend-selection tests**

Cover local/native-remote/tmux selection, host-scoped identity, explicit connect
intent, no start on restore/open/catalog, stale generations, one in-flight lane,
provider switching, and no silent fallback.

- [ ] **Step 2: Implement remote session backends**

Adapt M4 helper exchanges to `SessionClient` backend/connection contracts and keep
provider state outside editor/file/terminal-renderer modules.

- [ ] **Step 3: Write failing persistence tests**

Cover recent provider preference and selected opaque IDs without SSH credentials,
rendezvous secret, command bytes, tmux environment, or implicit reconnect/start.

- [ ] **Step 4: Implement secret-free provider persistence**

Persist only bounded UI preferences and last-known identifiers. Restore stopped or
disconnected projections with no remote side effect.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-app --test session_ui --test remote_ui --locked --offline
cargo test -p strukt-persistence --all-targets --locked --offline
cargo clippy -p strukt-app -p strukt-persistence --all-targets --locked --offline -- -D warnings
git add crates/strukt-app crates/strukt-persistence
git commit -m "feat: connect remote session providers"
```

## Task 7: Deliver the cohesive remote Sessions UI

**Files:**

- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/message.rs`
- Modify: `crates/strukt-shell/src/command.rs`
- Modify: `crates/strukt-app/tests/session_ui.rs`
- Modify: `crates/strukt-shell/tests/commands.rs`

- [ ] **Step 1: Write failing UI projection tests**

Cover recommended native/provider switcher, host/provider badges, capability-gated
actions, busy/unavailable explanations, stale banner, disabled input, reconnect,
repair, terminal fallback, Explorer availability, selection preservation, narrow
layout, semantic theme tokens, and accessibility labels.

- [ ] **Step 2: Implement remote Sessions presentation**

Reuse Focus + Context and the existing session hierarchy. Keep Explorer in the
activity rail and keyboard command surface while a session is focused.

- [ ] **Step 3: Add scriptable commands**

Register provider selection, attach/detach, reconnect, session/window/pane
selection, and Explorer focus commands with capability checks and deterministic
rejection reasons.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p strukt-app --test session_ui --locked --offline
cargo test -p strukt-shell --test commands --locked --offline
cargo clippy -p strukt-app -p strukt-shell --all-targets --locked --offline -- -D warnings
git add crates/strukt-app crates/strukt-shell
git commit -m "feat: add remote session workspace UI"
```

## Task 8: Prove real SSH persistence and provider fallback

**Files:**

- Create: `crates/strukt-remote/tests/remote_sessions_integration.rs`
- Create: `scripts/m5-remote-sessions-smoke.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `crates/strukt-app/tests/m4_remote_smoke.rs`
- Create: `crates/strukt-app/tests/m5_remote_sessions_smoke.rs`

- [ ] **Step 1: Write the deterministic end-to-end smoke**

Exercise fake SSH/helper/sessiond/tmux from Connections through native create,
multiple sessions/windows/panes, noisy isolation, disconnect, stale UI, delta/full
resync, app restart, stopped reboot restore, tmux discovery/attach/input/resize/
detach, Explorer access, and direct-terminal fallback.

- [ ] **Step 2: Add disposable real-OpenSSH integration**

Run packaged helper and sessiond under a disposable SSH user/environment, create
multiple real PTYs, record process/service identities, disconnect the SSH helper,
reconnect from a new helper, and prove identity, marker output, layout, and history
continuity.

- [ ] **Step 3: Add hosted matrix gates**

Run deterministic smoke on macOS, Ubuntu, and Windows. Run real OpenSSH,
native-service, and real tmux integration on Ubuntu when platform facilities are
available. Fail rather than silently skip a declared mandatory job.

- [ ] **Step 4: Preserve earlier regression smokes**

Run M1-M4 smokes and prove M4 workspace/file/editor/language/direct-terminal
behavior when sessions, tmux, or an incompatible helper is unavailable.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-app --test m4_remote_smoke --test m5_remote_sessions_smoke --locked --offline
cargo test -p strukt-remote --test remote_sessions_integration --locked --offline
bash scripts/m5-remote-sessions-smoke.sh
git add .github/workflows/ci.yml crates/strukt-app crates/strukt-remote scripts/m5-remote-sessions-smoke.sh
git commit -m "test: add M5 remote session integration"
```

## Task 9: Complete review and milestone evidence

**Files:**

- Create: `docs/evidence/m5-remote-persistent-sessions-validation.md`
- Modify: `docs/plans/0009-m5-remote-persistent-sessions.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`
- Modify: `README.md`

- [ ] **Step 1: Run the full release gate**

```bash
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline --quiet
cargo build -p strukt-app -p strukt-remote -p strukt-session --bins --locked --offline
bash scripts/m5-remote-sessions-smoke.sh
```

- [ ] **Step 2: Run the full-slice agentic review**

Review explicit-start policy, artifact trust, process detachment, service/socket
permissions, rendezvous secret retention, frame/queue/history bounds, stale
generation and response correlation, delta/resync correctness, noisy fairness,
writer lease, reboot behavior, tmux argv/injection/control parsing, capability
enforcement, persistence secrets, M4 fallback, UI blocking, accessibility,
cross-platform behavior, and scope drift. Resolve every critical or important
finding with a focused regression.

- [ ] **Step 3: Record exact evidence**

Document local and hosted commands/results, exact commit and run links, real SSH
process-identity proof, real/fake tmux results, protocol fallback, security review,
automated UI coverage, and human alpha gates. Do not claim a live EC2 or human
Windows walkthrough without evidence.

- [ ] **Step 4: Mark M5 complete**

Link spec, plan, issue, PR, and evidence in roadmap/tracker/README only after every
M5 acceptance criterion has direct evidence.

- [ ] **Step 5: Require exact final head and merge**

Update issue and PR verification, mark the PR ready only after the exact final head
passes macOS, Ubuntu, and Windows, then squash-merge under the merge policy.

## Final verification

M5 is complete only when native remote sessions survive real SSH/helper disconnect
and desktop restart, reconnect output is exact or explicitly resynced, concurrent
sessions remain fair and isolated, reboot restores stopped definitions without
command restart, real existing tmux sessions can be discovered and attached,
provider differences are enforced and visible, Explorer and M4 fallback remain
usable, security invariants hold, all earlier milestone smokes remain green, and
the exact final PR head passes the hosted platform matrix.
