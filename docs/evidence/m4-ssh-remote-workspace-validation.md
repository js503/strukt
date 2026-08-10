# M4 SSH Remote Workspace Validation

- Date: 2026-08-09
- Validated implementation head: `b07e00c`
- Issue: [#13](https://github.com/js503/strukt/issues/13)
- Pull request: [#14](https://github.com/js503/strukt/pull/14)
- Spec: [`../specs/0008-m4-ssh-remote-workspace.md`](../specs/0008-m4-ssh-remote-workspace.md)
- Plan: [`../plans/0008-m4-ssh-remote-workspace.md`](../plans/0008-m4-ssh-remote-workspace.md)

## Outcome

M4 adds a standard-OpenSSH remote workspace with secret-free recent connections,
explicit helper consent, terminal-only fallback, a versioned bounded CBOR helper
protocol, root-confined files, nested Quick Open discovery, multiline conditional
editing, search, read-only Git status, exact approved tasks, opt-in remote language
diagnostics, stale-generation rejection, and direct ephemeral SSH terminals.

The helper is a per-user Linux process reached only through protected SSH stdio. It
opens no port, requires no root access, and does not advertise persistent sessions;
remote PTY persistence and tmux interoperability remain M5 work.

## Local release gate

The implementation head passed on macOS:

```text
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline --quiet
cargo build -p strukt-app -p strukt-remote --bins --locked --offline
```

The app target reported `128 passed`; every workspace target passed with zero
failures. The only warning is the existing transitive `block 0.1.6`
future-incompatibility notice.

The deterministic fake-OpenSSH smoke passed with the real helper and app-level
coordinator:

```text
strukt M4 remote smoke: ssh, fallback, files, edit, search, git, task, language, disconnect, and reconnect passed
```

It verifies nested file discovery, revision-checked edit/conflict, search, Git,
bounded task output, an LSP initialize/did-open/publish-diagnostics exchange,
disconnect/reconnect generation isolation, terminal fallback, and absence of
`.strukt` workspace metadata.

## Disposable real OpenSSH validation

A localhost-only macOS `sshd` was started on an unprivileged port with generated
temporary Ed25519 host/client keys, a private temporary known-hosts file, password
authentication disabled, and strict host-key checking enabled. The opt-in ignored
test then used the real `/usr/bin/ssh` transport and real helper framing:

```text
cargo test -p strukt-remote --test native_ssh \
  disposable_real_openssh_runs_the_helper_protocol --locked --offline \
  -- --ignored --exact
test disposable_real_openssh_runs_the_helper_protocol ... ok
```

This proves real OpenSSH process, authentication, known-host, stdio, handshake,
capability negotiation, enumeration, and cleanup behavior. The same ignored test
also runs in the Ubuntu matrix against a disposable localhost-only Linux `sshd`
with generated non-production keys, strict known-host checking, and the built
helper. No EC2 host, cloud account, or production credential was used; the standard
OpenSSH alias path is vendor-independent and is the one an EC2 alias uses.

## Review findings resolved

The full-slice review found and fixed these material issues:

- remote process and language-server working directories followed symlinks outside
  the retained workspace root;
- Git/process/language subsystems did not all retain the helper's canonical root;
- Search, Git, tasks, and language transport existed only in the helper smoke and
  were not exposed in the native workspace;
- remote file discovery showed only root children rather than nested Quick Open
  candidates;
- remote editing used a single-line input instead of the native multiline editor;
- the initial Connections action row clipped helper installation at normal width;
- Windows conditional saves needed a dedicated same-volume atomic-replacement
  adapter for both workspace-root and nested parent directories;
- the Windows replacement-information buffer needed an explicit UTF-16 terminator;
  without it, aligned destination names could be published with a garbage suffix;
- the Git summary fixture used a legal Unix filename that Windows cannot create;
- Windows retained workspace-root handles prevent directory replacement, while
  Unix detects a replaced root before later operations;
- ConPTY may emit harmless startup bytes, so process-isolation coverage must assert
  that payloads remain bound to their process IDs rather than requiring silence;
- remote task completion could overtake final PTY reader output, and the smoke sent
  line-feed-only input rather than portable terminal Enter input;
- the language smoke required one read to contain an entire frame even though the
  byte-stream transport permits frames to span chunks.

Regressions cover symlink escapes, canonical helper roots, exact task approval,
capability gating, nested discovery, app-level Search/Git/task routing, real LSP
diagnostics, root and nested Windows atomic replacement, cross-platform Git
fixtures, platform-specific retained-root semantics, and the real OpenSSH opt-in
path. Bidirectional PTY payload tests cover process-ID isolation without assuming
platform-specific startup output. Remote task publication now performs a bounded
post-exit quiet-period drain before publishing final output, and the language smoke
accumulates bounded chunks before comparing complete payloads.

Review also confirmed separate validated SSH arguments, normal OpenSSH host-key and
agent behavior, exact artifact checksum consent, no public helper listener,
owner-private Unix persistence, bounded frames/queues/output, conditional atomic
saves, cancellation, generation rejection, helper process cleanup, secret-free
records, and no M5 persistence claim.

## Native macOS walkthrough

The release binary was wrapped in a temporary review-only `.app` and rendered as a
real Metal/wgpu Iced window. Connections remained visible beside Explorer and the
context panel; hostile aliases failed before SSH launch; dark/light themes worked.
The walkthrough exposed a clipped helper action, which was regrouped and
revalidated at the standard 988-by-768 window size.

The connected follow-up walkthrough could not run because the Mac became locked.
The connected paths are covered by deterministic app-level smoke and hosted native
builds, but final human keyboard, accessibility, IME, and Windows visual checks
remain explicit public-alpha gates rather than claimed passes.

## Hosted matrix

GitHub Actions run
[31357404357](https://github.com/js503/strukt/actions/runs/31357404357) is the
validated implementation-head matrix for macOS 14, Ubuntu 24.04, and Windows
Server 2022. Ubuntu additionally runs the disposable real-Linux-OpenSSH gate. The
exact completion-documentation head must pass the same three jobs before PR #14 is
marked ready and merged.

## Accepted alpha limitations

- M4 remote terminals are deliberately ephemeral. Persistent remote sessions,
  missed-output recovery, layouts, and tmux providers belong to M5.
- Remote Git is read-only and task arguments use an explicit JSON string array.
- Remote helper artifacts are currently Linux x86-64 or aarch64 only.
- A live EC2 account was not used. CI proves the same vendor-independent OpenSSH
  alias and real Linux helper path against an isolated Ubuntu host.
- Iced control-level accessibility, keyboard focus, IME, and human Windows visual
  certification remain public-alpha release gates.

These limitations do not permit implicit connection, helper installation, task or
language startup, credential persistence, disabled host verification, workspace
metadata, or a false persistent-session label.
