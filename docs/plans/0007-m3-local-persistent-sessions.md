# M3 Local Persistent Sessions Implementation Plan

> **Execution rule:** Follow the repository TDD, review, verification, and merge
> process. Keep the service and provider usable without the UI, and keep every
> blocking process or IPC operation outside the Iced update reducer.

**Goal:** Deliver named local sessions whose PTYs survive desktop-app closure,
reattach safely, and restore as stopped definitions after service or machine
restart.

**Architecture:** A new `strukt-session` crate owns the hierarchy, provider
contract, framed protocol, client, service reducer, and persistence models. A
repository-owned `strukt-sessiond` binary owns the existing terminal runtime and
communicates through authenticated per-user local IPC. `strukt-app` renders
immutable provider projections and schedules service work through tasks.

**Technology constraints:** Rust 2024, no unsafe workspace code, existing
`strukt-terminal` PTY/parser/grid/layout runtime, serde plus bounded versioned wire
models, HMAC-SHA256 authentication, OS-random service secrets, a safe local-socket
library for Unix-domain sockets and Windows named pipes, atomic application-data
persistence, Iced native UI, and deterministic repository fixtures.

**Spec:**
[`docs/specs/0007-m3-local-persistent-sessions.md`](../specs/0007-m3-local-persistent-sessions.md)

## Critical path

1. Freeze hierarchy, provider, protocol, security, and persistence contracts.
2. Build the real service around the M2 terminal runtime.
3. Build the reconnecting local provider client.
4. Integrate session/window projections and actions into the desktop app.
5. Migrate M2 terminal layouts without starting processes.
6. Prove detach/reattach, stopped reboot restoration, isolation, and cross-platform
   behavior through exact smokes and hosted CI.

## Parallelizable side tasks

The work remains single-owner in this session. Theme tokens, documentation, and
fixture expansion are bounded side tasks but are sequenced after their domain
contracts to avoid overlapping writes and speculative UI behavior.

## Task 1: Add Session Identity and Hierarchy Domain

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/strukt-session/Cargo.toml`
- Create: `crates/strukt-session/src/lib.rs`
- Create: `crates/strukt-session/src/id.rs`
- Create: `crates/strukt-session/src/catalog.rs`
- Create: `crates/strukt-session/tests/catalog.rs`

- [x] **Step 1: Write failing hierarchy tests**

Cover random stable session/window IDs, valid names, session/window/pane caps,
layout depth, active/focused invariants, independent revisions, lifecycle states,
and duplicate-name identity.

Run:

```bash
cargo test -p strukt-session --test catalog --locked --offline
```

Expected: fail because the crate and domain types do not exist.

- [x] **Step 2: Add the crate and opaque IDs**

Add 128-bit OS-random IDs with exact lowercase-hex serialization and strict parsing.
Reuse `TerminalPaneId` only at the terminal-runtime boundary; session protocol pane
IDs remain provider-owned to prevent accidental cross-provider aliasing.

- [x] **Step 3: Implement validated catalog mutations**

Implement create, rename, activate, duplicate, and remove for sessions/windows;
split/focus/ratio/close for panes; expected-revision checks; and normalized bounded
errors. A new session has one window and one stopped pane. Duplicates are stopped
definitions with empty runtime/history.

- [x] **Step 4: Verify domain invariants**

Run:

```bash
cargo test -p strukt-session --test catalog --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
```

Expected: pass.

- [x] **Step 5: Commit the hierarchy foundation**

```bash
git add Cargo.toml Cargo.lock crates/strukt-session
git commit -m "feat: add persistent session domain"
```

## Task 2: Add Provider Capabilities and Normalized Snapshots

**Files:**

- Create: `crates/strukt-session/src/provider.rs`
- Create: `crates/strukt-session/src/snapshot.rs`
- Create: `crates/strukt-session/tests/provider.rs`
- Modify: `crates/strukt-session/src/lib.rs`

- [x] **Step 1: Write failing provider contract tests**

Require capability-gated actions; bounded provider errors; immutable catalog,
session, window, and pane snapshots; output revision; attention/unread behavior;
and no process handles, commands, input, environment, or secrets in snapshots.

- [x] **Step 2: Implement capability and action models**

Define `SessionProviderCapabilities`, `SessionAction`, `ProviderError`, attach
leases, health, and provider metadata. Keep remote/tmux distinctions as capability
flags, not control-flow branches in app-facing types.

- [x] **Step 3: Implement structured terminal snapshot conversion**

Expose bounded grid/scrollback/cursor/title/mode projections from
`strukt-terminal` through owned serializable values. Add explicit snapshot byte and
row caps. Never serialize a terminal transport or live runtime.

- [x] **Step 4: Verify provider independence**

Run:

```bash
cargo test -p strukt-session --test provider --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
```

Expected: pass without depending on `strukt-app`.

- [x] **Step 5: Commit the provider contract**

```bash
git add crates/strukt-session crates/strukt-terminal
git commit -m "feat: define session provider snapshots"
```

## Task 3: Add Bounded Wire Protocol and Authentication

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/strukt-session/Cargo.toml`
- Create: `crates/strukt-session/src/auth.rs`
- Create: `crates/strukt-session/src/framing.rs`
- Create: `crates/strukt-session/src/protocol.rs`
- Create: `crates/strukt-session/tests/auth.rs`
- Create: `crates/strukt-session/tests/protocol.rs`
- Modify: `crates/strukt-session/src/lib.rs`

- [x] **Step 1: Write failing framing and authentication tests**

Cover fragmented and combined frames, every size limit, unknown kinds, malformed
CBOR, constant-time proof validation, wrong secret/instance/nonce, secret rotation,
monotonic request IDs, revision/generation guards, and zeroized secret ownership.

- [x] **Step 2: Select the CBOR and authentication dependencies**

Verify current official crate documentation and licenses. Add only maintained,
safe APIs compatible with Rust 1.97 and MIT/Apache licensing. Pin resolved versions
in `Cargo.lock`; do not introduce async runtime coupling into the domain crate. The
cross-platform local-socket dependency remains Task 5's platform decision.

- [x] **Step 3: Implement versioned bounded framing**

Use a 4-byte big-endian length and versioned CBOR payload. Separate ordinary and
snapshot limits, bound decoder retained bytes, reject ambiguous messages, and keep
raw frames out of error strings.

- [x] **Step 4: Implement the handshake**

Generate 256-bit service secrets from the OS random source. Prove possession with
HMAC-SHA256 over version, instance, endpoint identity, and client nonce. Compare
proofs in constant time, redact diagnostics, and zeroize secrets on drop.

- [x] **Step 5: Verify protocol bounds and dependency policy**

Run:

```bash
cargo test -p strukt-session --test auth --locked --offline
cargo test -p strukt-session --test protocol --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
```

Expected: pass; license and dependency rationale recorded in the spec or ADR if
needed.

- [x] **Step 6: Commit the protocol**

```bash
git add Cargo.toml Cargo.lock crates/strukt-session
git commit -m "feat: add authenticated session protocol"
```

## Task 4: Add Application-Data Catalog Persistence

**Files:**

- Create: `crates/strukt-session/src/store.rs`
- Create: `crates/strukt-session/tests/store.rs`
- Modify: `crates/strukt-session/src/lib.rs`

- [x] **Step 1: Write failing store tests**

Require atomic current/last-valid fallback, bounds, unknown-field round trips,
corrupt-current recovery, exclusive owner identity, sanitized bounded history, and
stopped-only restoration after a new service instance.

- [x] **Step 2: Implement versioned catalog records**

Persist only the approved definitions/presentation/history fields. Convert every
live lifecycle state to stopped on disk. Validate IDs, names, hierarchy, paths,
row/byte counts, and schema before returning a catalog.

- [x] **Step 3: Add explicit privacy regression tests**

Serialize representative sessions and assert that input, commands, environment,
secrets, process IDs, endpoint names, raw frames, clipboard, and selections are
absent. Verify no workspace `.strukt` path is touched.

- [x] **Step 4: Verify persistence**

Run:

```bash
cargo test -p strukt-session --test store --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
```

Expected: pass.

- [x] **Step 5: Commit persistence**

```bash
git add crates/strukt-session
git commit -m "feat: persist stopped session definitions"
```

## Task 5: Build the Cross-platform Local Endpoint

**Files:**

- Create: `crates/strukt-session/src/endpoint.rs`
- Create: `crates/strukt-session/src/rendezvous.rs`
- Create: `crates/strukt-session/tests/endpoint.rs`
- Modify: `crates/strukt-session/src/lib.rs`

- [x] **Step 1: Write failing endpoint tests**

Require per-user endpoint identity, no TCP listener, owner-only rendezvous,
exclusive service lock, stale-record recovery, authenticated round trip, client
disconnect isolation, queue bounds, and cleanup that cannot remove another service
instance's endpoint.

- [x] **Step 2: Implement platform-local sockets through safe APIs**

Use Unix-domain sockets on macOS/Linux and named pipes on Windows. Keep platform
details private to the endpoint module. Reject paths/names outside the application
data namespace and never place secrets in endpoint names or process arguments.

- [x] **Step 3: Implement rendezvous and service lock**

Atomically publish protocol version, service instance, endpoint identity, and a
secret reference after the listener is ready. Validate the existing owner through
an authenticated probe before treating a record as live.

- [x] **Step 4: Verify native and cross-target behavior**

Run:

```bash
cargo test -p strukt-session --test endpoint --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
cargo clippy -p strukt-session --all-targets --target x86_64-pc-windows-msvc --locked --offline -- -D warnings
```

Expected: native tests pass and Windows-specific code type-checks strictly.

- [x] **Step 5: Commit local IPC**

```bash
git add crates/strukt-session Cargo.toml Cargo.lock
git commit -m "feat: add local session IPC"
```

## Task 6: Build `strukt-sessiond` Around the Terminal Runtime

**Files:**

- Create: `crates/strukt-session/src/service.rs`
- Create: `crates/strukt-session/src/bin/strukt-sessiond.rs`
- Create: `crates/strukt-session/src/bin/session-fixture.rs`
- Create: `crates/strukt-session/tests/service.rs`
- Create: `crates/strukt-session/tests/native_service.rs`
- Modify: `crates/strukt-terminal/src/runtime.rs`
- Modify: `crates/strukt-terminal/src/lib.rs`

- [x] **Step 1: Write failing service reducer tests**

Require revision-safe hierarchy mutations, explicit start, no start on restore,
generation-scoped input/resize/output, detach without terminate, independent batch
restart results, bounded termination, fair draining, idle exit, and service-instance
stale rejection.

- [x] **Step 2: Extract reusable terminal runtime projections**

Add the smallest public read-only snapshot and attention hooks needed by the
service. Preserve M2 runtime tests and avoid exposing mutable parser/grid state.

- [x] **Step 3: Implement service request handling**

Authenticate before catalog access. Route mutations through one catalog reducer,
perform blocking spawn/terminate/store work outside its lock, and apply completions
only when instance/revision/generation still match.

- [x] **Step 4: Implement detach, fairness, and idle lifecycle**

Keep PTYs alive with zero clients, coalesce output revision events, drain panes
round-robin, persist debounced definitions/snapshots, and exit only after the
documented no-client/no-running-pane idle period.

- [x] **Step 5: Add real native service tests**

Launch `strukt-sessiond` with isolated app data and repository fixtures. Prove two
sessions remain isolated, detach preserves processes, reattach restores output,
terminate affects only the target, abrupt helper death restores stopped definitions,
and no child command restarts in a new instance.

- [x] **Step 6: Verify the daemon and fixture**

Run:

```bash
cargo test -p strukt-session --test service --locked --offline
cargo test -p strukt-session --test native_service --locked --offline
cargo build -p strukt-session --bin strukt-sessiond --locked --offline
cargo build -p strukt-session --bin session-fixture --locked --offline
```

Expected: pass without network or third-party shell multiplexer.

- [ ] **Step 7: Commit the native service**

```bash
git add crates/strukt-session crates/strukt-terminal
git commit -m "feat: add native persistent session service"
```

## Task 7: Build the Reconnecting Local Provider Client

**Files:**

- Create: `crates/strukt-session/src/client.rs`
- Create: `crates/strukt-session/tests/client.rs`
- Modify: `crates/strukt-session/src/lib.rs`

- [x] **Step 1: Write failing client tests**

Require lazy service start, authenticated attach, request routing, catalog refresh,
snapshot coalescing, stale response/event rejection, bounded reconnect backoff,
single writer lease, explicit detach, and frozen stale projection on transport loss.

- [x] **Step 2: Implement synchronous client state and executable jobs**

Keep connection state UI-independent. Return connect/request/poll jobs that callers
execute outside reducers. Never hide process launch, blocking IPC, retry sleeps, or
catalog mutation inside a state transition.

- [x] **Step 3: Implement service discovery and safe start**

Probe a rendezvous record first. Start only the exact repository helper path on an
explicit create/attach/restart action. Pass only the application-data path and
bootstrap handle/reference, never a secret or workspace path in arguments.

- [x] **Step 4: Verify reconnection and stale isolation**

Run:

```bash
cargo test -p strukt-session --test client --locked --offline
cargo clippy -p strukt-session --all-targets --locked --offline -- -D warnings
```

Expected: pass.

- [ ] **Step 5: Commit the local client**

```bash
git add crates/strukt-session
git commit -m "feat: add local session provider client"
```

## Task 8: Add App Session Coordination and Commands

**Files:**

- Modify: `crates/strukt-app/Cargo.toml`
- Create: `crates/strukt-app/src/session.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/terminal.rs`

- [x] **Step 1: Write failing app reducer tests**

Require no service/PTTY on workspace open, explicit lazy connect, session/window
selection, capability-gated commands, stale catalog rejection, detach on UI close,
transport-loss presentation, confirmation for destructive live actions, and no
focus leakage into editor/language/ephemeral terminal actions.

- [x] **Step 2: Add immutable app projections**

Store provider health, catalog revision, session list, active hierarchy, pending
request guards, confirmation state, and bounded failure details. Keep the provider
client and all blocking jobs outside serializable shell/workspace state.

- [x] **Step 3: Route stable commands**

Add stable session/window command messages and keyboard shortcuts. Translate active
pane input/resize only after verifying workspace, provider, service instance,
session, window, pane, and generation.

- [x] **Step 4: Integrate structured pane snapshots**

Render the active provider pane through the existing terminal widget without
duplicating parser/grid behavior in the app. Hidden sessions receive health and
revision updates but do not trigger unnecessary widget rebuilds.

- [x] **Step 5: Verify app coordination**

Run:

```bash
cargo test -p strukt-app --locked --offline session_
cargo clippy -p strukt-app --all-targets --locked --offline -- -D warnings
```

Expected: pass.

- [ ] **Step 6: Commit app coordination**

```bash
git add crates/strukt-app
git commit -m "feat: coordinate persistent local sessions"
```

## Task 9: Build the Sessions and Windows Interface

**Files:**

- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-theme/src/tokens.rs`
- Modify: `crates/strukt-theme/tests/builtin_themes.rs`

- [x] **Step 1: Write failing view and theme tests**

Require persistent file-browser access, session list labels and counts, window
strip, live/stopped/stale/unread/attention semantics, exact destructive dialog
labels, keyboard focus order, compact layout, and distinct light/dark tokens.

- [x] **Step 2: Add semantic tokens**

Add provider/session live, stopped, stale, unread, attention, active, and selected
tokens with non-color indicators and documented contrast behavior.

- [x] **Step 3: Build the Sessions activity surface**

Keep the stable activity rail and file explorer. Add a session list with provider,
state, counts, and actions; a compact window strip; and active session/window labels
in the status area. Do not let output steal focus or replace files.

- [x] **Step 4: Build confirmation and failure surfaces**

Name exact targets and running-pane counts. Expose retry/reconnect, copy bounded
failure, terminate, and remove actions according to provider capabilities.

- [x] **Step 5: Verify responsive layout and focus**

Run:

```bash
cargo test -p strukt-app --locked --offline session_view_
cargo test -p strukt-theme --locked --offline
```

Expected: pass in narrow and wide layout projections.

- [ ] **Step 6: Commit the interface**

```bash
git add crates/strukt-app crates/strukt-theme
git commit -m "feat: show persistent sessions and windows"
```

## Task 10: Migrate M2 Terminal Layouts Safely

**Files:**

- Modify: `crates/strukt-persistence/src/terminal_store.rs`
- Modify: `crates/strukt-persistence/src/workspace_store.rs`
- Create: `crates/strukt-persistence/src/session_store.rs`
- Create: `crates/strukt-persistence/tests/session_store.rs`
- Modify: `crates/strukt-persistence/src/lib.rs`
- Modify: `crates/strukt-app/src/session.rs`

- [x] **Step 1: Write failing migration tests**

Require one `Local` session, terminal-tab to window mapping, exact valid layouts and
working directories, stopped panes, idempotency, M3-wins conflict behavior,
unknown sibling preservation, and no helper/process start.

- [x] **Step 2: Implement versioned M3 contribution metadata**

Persist only workspace presentation linkage and selected session/provider IDs in
the existing workspace snapshot. Keep the authoritative live/stopped catalog in
application data owned by the session service.

- [x] **Step 3: Implement explicit migration job**

Convert the old contribution into a stopped service catalog only on first explicit
M3 use. Do not mutate the workspace or delete old data until the service catalog
and next workspace snapshot both save successfully.

- [x] **Step 4: Verify persistence isolation**

Run:

```bash
cargo test -p strukt-persistence --test session_store --locked --offline
cargo test -p strukt-app --locked --offline session_migration_
```

Expected: pass with no `.strukt` metadata and no runtime content in workspace state.

- [ ] **Step 5: Commit migration**

```bash
git add crates/strukt-persistence crates/strukt-app
git commit -m "feat: migrate terminal layouts to sessions"
```

## Task 11: Add Deterministic M3 Smoke and Hosted Matrix

**Files:**

- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-session/src/bin/session-fixture.rs`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Write failing smoke parser and orchestration tests**

Require exact `--session-smoke <existing-root>`, isolated application data,
repository helper/fixture paths, outer timeout, two-session isolation, detach and
reattach, output/layout verification, terminate isolation, stopped service restart,
no `.strukt`, and exact success text.

- [ ] **Step 2: Implement the smoke coordinator**

Print only:

```text
strukt M3 session smoke: hierarchy, isolation, detach, reattach, history, termination, and stopped restore passed
```

on complete success. Bound every child process and clean up the isolated helper on
all error paths.

- [ ] **Step 3: Extend CI on all three platforms**

Build `strukt-sessiond` and `session-fixture`, run the smoke on macOS 14, Ubuntu
24.04, and Windows Server 2022, and retain the existing M1/M2 checks. Add an outer
timeout and exact marker/no-metadata assertions for both shell families.

- [ ] **Step 4: Run the local release gate**

Run:

```bash
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
cargo build -p strukt-session --bin strukt-sessiond --locked --offline
cargo build -p strukt-session --bin session-fixture --locked --offline
cargo build -p strukt-app --locked --offline
cargo run -p strukt-app -- --session-smoke "$fixture"
```

Expected: every command passes, the exact marker is printed, and the workspace
contains no `.strukt` metadata.

- [ ] **Step 5: Commit the smoke and CI gate**

```bash
git add crates/strukt-app crates/strukt-session .github/workflows/ci.yml
git commit -m "test: validate persistent local sessions"
```

## Task 12: Review, Validate, and Close M3

**Files:**

- Modify: `README.md`
- Create: `docs/evidence/m3-local-persistent-sessions-validation.md`
- Modify: `docs/plans/0007-m3-local-persistent-sessions.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`
- Modify: `docs/decisions/0001-native-ui-framework.md`

- [ ] **Step 1: Complete the native macOS walkthrough**

Exercise session/window/pane hierarchy, detach after closing the UI, reattach,
unread/attention, service loss, stale display, restart/terminate confirmation,
file-browser access, keyboard focus, themes, stopped restoration, and no workspace
metadata. Record Iced accessibility/IME limits honestly.

- [ ] **Step 2: Run full-slice agentic review**

Review local endpoint ownership and ACL assumptions, authentication, secret
lifetime, framing/queue bounds, service lock races, process inheritance and cleanup,
detach versus terminate, stale instances/revisions/generations, persistence privacy,
corruption fallback, path replacement, output fairness, attention state, M2
migration, capability isolation, app focus, cross-platform helper behavior, and
M4/M5 boundary drift. Resolve all critical and important findings with focused
regressions.

- [ ] **Step 3: Record exact local and hosted evidence**

Require the implementation and final documentation heads to pass macOS 14, Ubuntu
24.04, and Windows Server 2022. Record run/job links, native service/smoke markers,
manual results, review findings, and accepted public-alpha limitations.

- [ ] **Step 4: Update milestone artifacts**

Mark M3 complete in roadmap/tracker, link spec/plan/issue/PR/evidence, update README
and ADR 0001, and keep M4 then M5 as the remaining public-alpha implementation path.

- [ ] **Step 5: Commit completion evidence**

```bash
git add README.md docs/decisions/0001-native-ui-framework.md \
  docs/evidence/m3-local-persistent-sessions-validation.md \
  docs/plans/0007-m3-local-persistent-sessions.md docs/roadmap.md docs/tracker.md
git commit -m "docs: complete M3 local persistent sessions"
```

- [ ] **Step 6: Require exact final head and merge**

Update the issue and PR with verification and substantive review findings. Mark the
PR ready only after the exact final head is green, then squash-merge under
`docs/process/merge-policy.md`.

## Final Verification

M3 is not complete until every acceptance criterion in
`docs/specs/0007-m3-local-persistent-sessions.md` has direct evidence, all review
findings are resolved or explicitly accepted, the issue and PR link every required
artifact, and the exact final PR head is green on macOS, Ubuntu, and Windows. M4
begins from merged `main`; neither workspace restore nor application launch may
start a session helper or arbitrary pane process.
