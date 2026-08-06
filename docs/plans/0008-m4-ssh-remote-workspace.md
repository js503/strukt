# M4 SSH Remote Workspace Implementation Plan

> Execute each task test-first. Keep OpenSSH, remote protocol, helper behavior,
> workspace projections, and Iced integration behind separate contracts. Do not
> absorb M5 persistent-session behavior into this milestone.

**Goal:** Deliver a first-class SSH-backed Linux workspace with standard OpenSSH
configuration, explicit helper consent, direct terminal fallback, remote files and
editing, Quick Open, search, Git summary, approved tasks, diagnostics, language
transport, stale reconnect behavior, and exact macOS/Windows/Linux verification.

**Architecture:** Add a `strukt-remote` crate that owns validated connection
identity, OpenSSH process construction, connection state, helper framing/protocol,
root-confined remote operations, and immutable client projections. Add a separate
`strukt-remote` helper binary in that crate and a repository-owned `fake-ssh`
fixture. Extend persistence with secret-free remote records. The app adapts remote
snapshots into existing file, editor, terminal, language, Problems, and shell
surfaces without teaching those surfaces SSH details.

**Tech stack:** Rust 2024, platform OpenSSH process, CBOR framing through
`ciborium`, BLAKE3 revisions/checksums, existing `portable-pty`, `ignore`, Iced,
serde persistence, strict workspace lint, and deterministic repository fixtures.

**Spec:** [`../specs/0008-m4-ssh-remote-workspace.md`](../specs/0008-m4-ssh-remote-workspace.md)

---

## Execution constraints

- [ ] Follow red-green-refactor for every behavior change.
- [ ] Run focused tests after each step and strict format/lint before every commit.
- [ ] Never invoke a shell to construct the local OpenSSH process.
- [ ] Never interpolate host aliases or remote paths into remote shell text.
- [ ] Preserve normal OpenSSH host verification and authentication policy.
- [ ] Keep all frames, queues, streams, retries, operations, and diagnostics bounded.
- [ ] Opening/restoring a remote workspace performs no connection or process side
      effect.
- [ ] Keep direct terminal fallback functional whenever OpenSSH itself works.
- [ ] Do not implement persistent remote sessions or tmux before M5.
- [ ] Use no production credentials in fixtures, logs, commits, or evidence.

## Task 1: Establish remote identities and connection state

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/strukt-remote/Cargo.toml`
- Create: `crates/strukt-remote/src/lib.rs`
- Create: `crates/strukt-remote/src/target.rs`
- Create: `crates/strukt-remote/src/state.rs`
- Create: `crates/strukt-remote/tests/target.rs`
- Create: `crates/strukt-remote/tests/state.rs`

- [ ] **Step 1: Write failing target tests**

Cover stable connection IDs, opaque valid aliases, rejection of empty, leading
hyphen, NUL, line breaks, and oversized aliases, normalized absolute or home-rooted
Linux roots, rejection of escapes, and distinct identities for different aliases
or roots.

- [ ] **Step 2: Implement target values**

Add `ConnectionId`, `SshAlias`, `RemoteRoot`, `RemoteWorkspaceId`, and typed
validation errors. Keep display labels separate from transport identity.

- [ ] **Step 3: Write failing state-machine tests**

Cover disconnected, connecting, terminal-only, negotiation, ready, stale,
reconnecting, failed, disconnecting, explicit retry, bounded exponential backoff,
generation changes, cancellation, and invalid transitions.

- [ ] **Step 4: Implement state and immutable projections**

Add explicit health, recovery actions, capability summaries, operation IDs,
generation/sequence cursors, and deterministic backoff without any Iced dependency.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-remote --test target --test state --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add Cargo.toml Cargo.lock crates/strukt-remote
git commit -m "feat: add remote workspace identities"
```

## Task 2: Discover OpenSSH configuration safely

**Files:**

- Create: `crates/strukt-remote/src/config.rs`
- Create: `crates/strukt-remote/src/ssh.rs`
- Create: `crates/strukt-remote/tests/config.rs`
- Create: `crates/strukt-remote/tests/ssh.rs`

- [ ] **Step 1: Write failing config-discovery tests**

Use disposable config trees to cover literal aliases, multiple `Host` tokens,
wildcards, negation, comments, quoting, bounded recursive `Include`, glob ordering,
cycles, unreadable files, duplicate aliases, and explicit aliases independent of
discovery.

- [ ] **Step 2: Implement best-effort alias discovery**

Return deterministic discovered aliases plus bounded warnings. Never treat a
wildcard pattern as a selectable alias and never block an explicit alias because
discovery failed.

- [ ] **Step 3: Write failing OpenSSH construction tests**

Cover executable resolution, Windows fallback path, `ssh -V`, `ssh -G`, probe,
terminal, helper, cancellation, fixed options, separate arguments, hostile aliases,
bounded diagnostics, deadlines, environment policy, and no shell invocation.

- [ ] **Step 4: Implement the typed OpenSSH adapter**

Add injectable process execution, config preview parsing, executable metadata,
argument-vector builders, operation deadlines, cancellation, and structured exit
results. Preserve normal OpenSSH security defaults.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-remote --test config --test ssh --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add crates/strukt-remote
git commit -m "feat: add standard OpenSSH transport"
```

## Task 3: Define the bounded helper protocol

**Files:**

- Create: `crates/strukt-remote/src/framing.rs`
- Create: `crates/strukt-remote/src/protocol.rs`
- Create: `crates/strukt-remote/src/capability.rs`
- Create: `crates/strukt-remote/tests/framing.rs`
- Create: `crates/strukt-remote/tests/protocol.rs`

- [ ] **Step 1: Write failing framing tests**

Cover magic preface, partial reads/writes, zero/oversized lengths, truncation,
multiple frames, allocation bounds, trailing data, EOF, invalid CBOR, and clear
protocol errors without panics.

- [ ] **Step 2: Implement bounded CBOR framing**

Use fixed maximums and checked conversions. Keep stderr outside the protocol stream.

- [ ] **Step 3: Write failing protocol tests**

Cover major mismatch, minor/capability intersection, nonce echo, build target,
limits, stable request IDs, duplicate IDs, stream sequence, credit, completion,
cancellation, post-cancel data, typed filesystem/process errors, ignored extensible
fields, and rejection of invalid state transitions.

- [ ] **Step 4: Implement protocol types and negotiation**

Define handshake, capability set, request/response/event envelopes, filesystem,
search, Git, process, language stream, watch, cancellation, and protocol errors.
Do not add persistent-session messages.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-remote --test framing --test protocol --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add crates/strukt-remote
git commit -m "feat: define the remote helper protocol"
```

## Task 4: Implement the root-confined helper filesystem

**Files:**

- Create: `crates/strukt-remote/src/path.rs`
- Create: `crates/strukt-remote/src/filesystem.rs`
- Create: `crates/strukt-remote/src/helper.rs`
- Create: `crates/strukt-remote/src/bin/strukt-remote.rs`
- Create: `crates/strukt-remote/tests/path.rs`
- Create: `crates/strukt-remote/tests/filesystem.rs`
- Create: `crates/strukt-remote/tests/helper.rs`

- [ ] **Step 1: Write failing root-confinement tests**

Cover absolute child paths, `.`, `..`, empty segments, repeated separators, NUL,
symlink escape, symlink replacement races, root replacement, non-UTF-8 entries on
Unix, Windows-like prefixes as hostile input, and valid nested paths.

- [ ] **Step 2: Implement confined remote paths**

Open the canonical approved root, validate lexical paths, use capability-oriented
directory access where possible, verify followed symlinks remain inside root, and
return escaped metadata for names not editable as UTF-8.

- [ ] **Step 3: Write failing filesystem behavior tests**

Cover deterministic paged listings, metadata/revisions, bounded chunked reads,
binary and UTF-8 reporting, ignored/hidden discovery, Quick Open streaming, search
limits/cancellation, conditional atomic saves, mode preservation, conflicts,
unknown outcome hooks, watch sequences, overflow, and resync.

- [ ] **Step 4: Implement helper filesystem operations**

Reuse local discovery semantics without exposing local paths. Apply BLAKE3-backed
revision identity where needed and private atomic sibling writes.

- [ ] **Step 5: Add the stdio helper loop**

Implement handshake, bounded dispatch, independent operation cancellation, stream
credit, graceful EOF, and deterministic shutdown in `strukt-remote --stdio`.

- [ ] **Step 6: Verify and commit**

```bash
cargo test -p strukt-remote --test path --test filesystem --test helper --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add crates/strukt-remote
git commit -m "feat: add the confined remote helper"
```

## Task 5: Add ephemeral remote processes, Git, and language streams

**Files:**

- Create: `crates/strukt-remote/src/process.rs`
- Create: `crates/strukt-remote/src/git.rs`
- Create: `crates/strukt-remote/src/language.rs`
- Create: `crates/strukt-remote/tests/process.rs`
- Create: `crates/strukt-remote/tests/git.rs`
- Create: `crates/strukt-remote/tests/language.rs`
- Modify: `crates/strukt-remote/src/helper.rs`

- [ ] **Step 1: Write failing process tests**

Cover exact executable/argument spawning without a shell, explicit shell mode,
workspace cwd confinement, sanitized environment overrides, stdin, stdout/stderr
separation, Unicode, resize capability, output credit, cancellation, deadline,
exit status, process-tree cleanup, and concurrent operation fairness.

- [ ] **Step 2: Implement bounded ephemeral processes**

Own children inside the helper, route independent streams, apply cancellation and
deadlines, and guarantee helper exit does not leave unapproved process trees.

- [ ] **Step 3: Write failing Git-summary tests**

Cover non-repository roots, branch/detached state, modified/staged/untracked counts,
paths with spaces/newlines, bounded output, Git absence, cancellation, and no
write-side Git commands.

- [ ] **Step 4: Implement read-only Git summary**

Invoke Git with fixed arguments and parse machine-oriented output defensively.

- [ ] **Step 5: Write failing language-stream tests**

Launch the repository language fixture remotely, carry LSP bytes unchanged, bound
queues, cancel/restart explicitly, preserve URI/path translation at the workspace
boundary, and reject implicit launch.

- [ ] **Step 6: Implement remote language transport**

Expose a transport adapter usable by `strukt-language` without embedding SSH or
helper types into the language core.

- [ ] **Step 7: Verify and commit**

```bash
cargo test -p strukt-remote --test process --test git --test language --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add crates/strukt-remote
git commit -m "feat: add remote development processes"
```

## Task 6: Implement helper installation and the real client

**Files:**

- Create: `crates/strukt-remote/src/install.rs`
- Create: `crates/strukt-remote/src/client.rs`
- Create: `crates/strukt-remote/src/bin/fake-ssh.rs`
- Create: `crates/strukt-remote/tests/install.rs`
- Create: `crates/strukt-remote/tests/client.rs`
- Create: `crates/strukt-remote/tests/native_ssh.rs`

- [ ] **Step 1: Write failing installer tests**

Cover Linux OS/architecture detection, exact artifact selection, missing artifacts,
local checksum mismatch, exact consent scope, fixed install path, `umask 077`,
temporary cleanup, atomic versioned rename, remote checksum success/failure,
non-root behavior, incompatible versions, and no overwrite of active helpers.

- [ ] **Step 2: Implement the installer plan and executor**

Separate pure install planning from OpenSSH execution. Stream helper bytes through
stdin and accept only structured results from the fixed bootstrap.

- [ ] **Step 3: Write failing client lifecycle tests**

Use `fake-ssh` to cover config preview, terminal-only connection, helper handshake,
capability snapshots, every helper operation, disconnect during transfer, stale
generation, reconnect, old-result rejection, bounded retry, helper crash, invalid
frame, stderr diagnostics, terminal fallback, and explicit disconnect.

- [ ] **Step 4: Implement the remote client**

Own OpenSSH/helper processes off the UI thread, serialize writes, read frames,
enforce in-flight limits, publish immutable projections, cancel predictably, and
preserve the last snapshot only as stale state.

- [ ] **Step 5: Add native OpenSSH contract coverage**

When an opt-in disposable SSH endpoint is configured, exercise the installed
OpenSSH executable. Otherwise test executable/config behavior without contacting a
user host. Never read or print production key material.

- [ ] **Step 6: Verify and commit**

```bash
cargo test -p strukt-remote --test install --test client --test native_ssh --locked --offline
cargo clippy -p strukt-remote --all-targets --locked --offline -- -D warnings
git add crates/strukt-remote
git commit -m "feat: connect the remote helper over OpenSSH"
```

## Task 7: Persist secret-free remote records

**Files:**

- Modify: `crates/strukt-persistence/src/lib.rs`
- Create: `crates/strukt-persistence/src/remote_store.rs`
- Create: `crates/strukt-persistence/tests/remote_store.rs`
- Modify: `crates/strukt-persistence/src/workspace_store.rs`
- Modify: `crates/strukt-persistence/tests/workspace_store.rs`

- [ ] **Step 1: Write failing persistence tests**

Cover versioned connection records, recent roots, helper metadata, deterministic
ordering, atomic writes, mode protection, corruption fallback, schema migration,
forget, opaque unknown fields, and explicit absence of keys, passphrases, agent
tokens, passwords, raw environment, and protocol payloads.

- [ ] **Step 2: Implement the remote store**

Store only approved connection presentation metadata in the user application-data
area. Keep remote contributions optional so disabling the module cannot make a
workspace unreadable.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p strukt-persistence --test remote_store --test workspace_store --locked --offline
cargo clippy -p strukt-persistence --all-targets --locked --offline -- -D warnings
git add crates/strukt-persistence
git commit -m "feat: persist remote workspace records"
```

## Task 8: Integrate remote workspaces into the native app

**Files:**

- Modify: `crates/strukt-app/Cargo.toml`
- Create: `crates/strukt-app/src/remote.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/workspace.rs`
- Modify: `crates/strukt-app/src/editor.rs`
- Modify: `crates/strukt-app/src/language.rs`
- Modify: `crates/strukt-app/src/terminal.rs`
- Modify: `crates/strukt-shell/src/activity.rs`
- Modify: `crates/strukt-theme/src/tokens.rs`
- Modify: `crates/strukt-theme/tests/builtin_themes.rs`

- [ ] **Step 1: Write failing app-state tests**

Cover Connections activity, alias add/forget, recent roots, no-side-effect restore,
explicit connect/disconnect/retry, exact install consent, terminal fallback, stale
snapshots, generation isolation, capability-gated actions, bounded messages,
command IDs, focus restoration, file explorer availability, and remote labels.

- [ ] **Step 2: Add the remote coordinator**

Adapt remote client projections into app messages and existing workspace/editor,
language, terminal, Problems, and persistence contracts. Keep all blocking work in
Iced tasks or worker threads and reject stale completions.

- [ ] **Step 3: Write failing remote file/editor tests**

Cover tree paging/expansion, Quick Open, file read, binary/invalid UTF-8, dirty
editing, conditional save, conflict, unknown outcome, remote search, watcher
overflow, reload, and no `.strukt` metadata.

- [ ] **Step 4: Connect existing surfaces to remote providers**

Share editor and presentation logic while dispatching filesystem/process behavior
through the active workspace target. Do not convert remote paths into local paths.

- [ ] **Step 5: Write failing task/language/terminal tests**

Cover exact remote task approval scope, no implicit launch, remote language fixture,
diagnostics host labels, explicit restart/cancel, direct OpenSSH terminal input and
resize, helper failure while terminal remains offered, and no persistence claim.

- [ ] **Step 6: Integrate task, language, and terminal adapters**

Keep M2 behavior and limits. A direct terminal ends at disconnect and is never
shown as an M3/M5 persistent session.

- [ ] **Step 7: Verify and commit**

```bash
cargo test -p strukt-app --lib --locked --offline
cargo test -p strukt-theme --test builtin_themes --locked --offline
cargo clippy -p strukt-app -p strukt-shell -p strukt-theme --all-targets --locked --offline -- -D warnings
git add crates/strukt-app crates/strukt-shell crates/strukt-theme
git commit -m "feat: add the remote workspace interface"
```

## Task 9: Build the functional Connections and boundary UI

**Files:**

- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/remote.rs`
- Modify: `crates/strukt-theme/src/tokens.rs`

- [ ] **Step 1: Write failing view-model tests**

Cover connected, connecting, terminal-only, helper negotiation, ready, stale,
failed, and disconnected labels without color-only meaning; host boundary labels in
header/explorer/editor/terminal/Problems/status; exact install summary; disabled
capabilities; narrow width; both themes; and Connections while explorer stays open.

- [ ] **Step 2: Implement the Connections view**

Add discovered/explicit hosts, recent roots, Open, Terminal, Reconnect, Disconnect,
Forget, Install/Repair Helper, diagnostics, exact confirmation, status, and keyboard
focus using existing native controls and semantic tokens.

- [ ] **Step 3: Implement remote boundary chrome**

Apply host and stale/degraded labels consistently, keep the center dominant, retain
one-shortcut explorer access, and avoid adding visual complexity outside the
approved mockup direction.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p strukt-app --lib --locked --offline
cargo test -p strukt-theme --test builtin_themes --locked --offline
cargo clippy -p strukt-app -p strukt-theme --all-targets --locked --offline -- -D warnings
git add crates/strukt-app crates/strukt-theme
git commit -m "feat: add remote connection workspace UI"
```

## Task 10: Add deterministic M4 smoke and hosted coverage

**Files:**

- Create: `crates/strukt-app/src/remote_smoke.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`

- [ ] **Step 1: Write failing launch-mode tests**

Require exact `--remote-smoke <root>` arguments, an existing fixture root, no
interactive fallback on malformed input, and the exact success marker.

- [ ] **Step 2: Implement the deterministic smoke**

Use the real helper through `fake-ssh` to prove config preview, terminal-only
fallback, handshake/capabilities, remote listing, Quick Open, edit/save/conflict,
search, Git summary, approved task, language diagnostics, disconnect/stale state,
reconnect/generation isolation, helper failure fallback, and no workspace metadata.

- [ ] **Step 3: Add the matrix smoke**

Build `strukt-remote` and `fake-ssh`, run the smoke on macOS, Windows, and Linux,
retain all earlier milestone smokes, and add opt-in disposable real-SSH coverage
without requiring production secrets.

- [ ] **Step 4: Document local execution**

Add exact commands, helper artifact expectations, security behavior, and the M4/M5
boundary to README.

- [ ] **Step 5: Verify and commit**

```bash
cargo build -p strukt-remote --bin strukt-remote --bin fake-ssh --locked --offline
cargo build -p strukt-app --locked --offline
fixture="$(mktemp -d)"
cargo run -p strukt-app --locked --offline -- --remote-smoke "$fixture"
test ! -e "$fixture/.strukt"
git add .github/workflows/ci.yml README.md crates/strukt-app
git commit -m "test: validate SSH remote workspaces"
```

Expected marker:

```text
strukt M4 remote smoke: ssh, fallback, files, edit, search, git, task, language, disconnect, and reconnect passed
```

## Task 11: Complete review, native walkthroughs, and milestone evidence

**Files:**

- Create: `docs/evidence/m4-ssh-remote-workspace-validation.md`
- Modify: `docs/plans/0008-m4-ssh-remote-workspace.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`
- Modify: `docs/decisions/0001-native-ui-framework.md`
- Modify: `README.md`

- [ ] **Step 1: Run full local release gate**

```bash
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline --quiet
cargo build -p strukt-app -p strukt-remote --bins --locked --offline
```

- [ ] **Step 2: Complete native macOS and Windows walkthroughs**

Exercise Connections, host/root entry, explicit connect, exact helper consent,
remote tree/Quick Open/editor/search/Problems/task/terminal, boundary labels, both
themes, explorer shortcut, stale disconnect, retry, helper repair, terminal-only
fallback, keyboard focus, accessibility, IME, and narrow-window behavior. Record
what is automated, human-verified, or an explicit alpha gate.

- [ ] **Step 3: Run full-slice agentic review**

Review command construction, config parsing, host verification, authentication
prompts, helper artifact trust, bootstrap injection, permissions, root/symlink
confinement, TOCTOU, frame/queue bounds, cancellation, process cleanup, secret
persistence, stale generations, save unknown outcomes, watch resync, reconnect
storms, output fairness, UI blocking, capability isolation, M2/M3 regression, and
M5 boundary drift. Resolve every critical or important finding with a focused
regression.

- [ ] **Step 4: Record exact local, real-SSH, and hosted evidence**

Document the deterministic smoke, native walkthroughs, disposable real-OpenSSH
result, matrix run/job links, security findings, helper install behavior, and
accepted alpha limitations. Do not claim live EC2 verification without evidence.

- [ ] **Step 5: Mark M4 complete**

Link spec, plan, issue, PR, and evidence in roadmap/tracker; update README and ADR;
keep M5 as the remaining remote-session implementation milestone.

- [ ] **Step 6: Commit completion evidence**

```bash
git add README.md docs/decisions/0001-native-ui-framework.md \
  docs/evidence/m4-ssh-remote-workspace-validation.md \
  docs/plans/0008-m4-ssh-remote-workspace.md docs/roadmap.md docs/tracker.md
git commit -m "docs: complete M4 SSH remote workspaces"
```

- [ ] **Step 7: Require exact final head and merge**

Update the issue and PR with verification and substantive review findings. Mark the
PR ready only after the exact final head is green on macOS, Ubuntu, and Windows,
then squash-merge under `docs/process/merge-policy.md`. Begin M5 only from merged
`main`.

## Final verification

M4 is complete only when every acceptance criterion in
`docs/specs/0008-m4-ssh-remote-workspace.md` has direct evidence, deterministic
fixtures and an actual disposable OpenSSH path agree, all important review findings
are resolved or explicitly accepted, helper consent and terminal fallback work,
credentials remain absent from persistence, earlier M1-M3 smokes remain green, and
the exact final PR head passes macOS 14, Ubuntu 24.04, and Windows Server 2022.
