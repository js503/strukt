# M3 Local Persistent Sessions Validation

- Date: 2026-08-02
- Implementation head: `f081736`
- Issue: [#11](https://github.com/js503/strukt/issues/11)
- Pull request: [#12](https://github.com/js503/strukt/pull/12)
- Spec: [`../specs/0007-m3-local-persistent-sessions.md`](../specs/0007-m3-local-persistent-sessions.md)
- Plan: [`../plans/0007-m3-local-persistent-sessions.md`](../plans/0007-m3-local-persistent-sessions.md)

## Outcome

M3 implements a native local persistent-session provider and a per-user
`strukt-sessiond` helper. Named sessions own windows, split panes, PTYs, bounded
screen history, requested terminal sizes, and attention state independently of the
desktop process. Closing or detaching the app does not terminate live panes.
Starting a workspace alone does not start the helper or a pane.

The provider boundary, identifiers, capabilities, authenticated framed protocol,
stale-generation guards, stopped-only persistence, and immutable UI projections
remain independent of Iced. The same normalized contract is the starting point for
the remote native and tmux providers in M5.

## Local release gate

The implementation head passed on macOS:

```text
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline --quiet
```

The workspace test gate exited `0`; the app unit target reported `123 passed`, and
all crate, integration, protocol, endpoint, service, persistence, renderer, LSP,
filesystem, and native-service targets passed with zero failures. The only emitted
toolchain warning is the transitive `block 0.1.6` future-incompatibility notice.

The exact deterministic native smoke also passed:

```text
cargo build -p strukt-session --bin strukt-sessiond --bin session-fixture --locked --offline
cargo build -p strukt-app --locked --offline
cargo run -p strukt-app --locked --offline -- --session-smoke /private/tmp/strukt-m3-f081736.osuvGo
strukt M3 session smoke: hierarchy, isolation, detach, reattach, history, termination, and stopped restore passed
```

The smoke creates two isolated sessions, runs repository-owned PTY fixtures,
detaches and reconnects, verifies exact output, terminates one complete session,
proves its sibling remains usable, kills and restarts the helper, verifies two
stopped definitions plus bounded historical output, and creates no `.strukt`
workspace metadata.

## Review findings resolved

The full-slice review found and resolved these important defects:

- pane termination and resize ignored the request's expected catalog revision;
- the provider advertised session termination but exposed only pane termination;
- session restart/terminate batch actions and confirmations were absent;
- requested terminal sizes were not persisted or copied into stopped duplicates;
- unread and attention state was not included in catalog projections and viewing
  the newest active snapshot did not clear it;
- UI actions were not gated by normalized provider capabilities;
- adding the M3 actions clipped the center surface when both sidebars were visible
  at a normal 988-pixel window width;
- new wire operations required an explicit protocol-version change.

Focused regressions now cover stale destructive requests, requested-size
persistence, session-level termination, batch results, provider attention
summaries, native daemon recovery, and the exact end-to-end smoke.

Security review confirmed a non-network local endpoint, generated endpoint
identity, HMAC-SHA256 authentication, constant-time verification, per-service
secret rotation, owner-only Unix directory/socket/record/secret permissions,
Windows owner/System pipe SDDL, bounded CBOR frames and queues, single controlling
client, OS-released service locking, atomic stopped-only persistence, and
owner-checked rendezvous cleanup. Windows secret and record files rely on the
current user's private application-data directory ACL in addition to the
authenticated owner-only pipe.

## Native macOS walkthrough

The release binary was wrapped in a temporary review-only `.app` bundle and opened
as a real Metal/wgpu Iced window. The Sessions activity rendered alongside the
real file explorer and context surface. `Command-B` hid and restored the file
browser without replacing Sessions, proving the required one-shortcut access. The
first walkthrough exposed clipped session action rows; those rows were regrouped
into narrow-safe session, window, pane, input, and size controls and rebuilt.

The deterministic native smoke provides the lifecycle proof that the macOS
accessibility bridge could not drive reliably: hierarchy, running PTYs,
detach/reattach, session termination, daemon loss, historical output, and
stopped-only recovery all passed against the same executable head.

## Hosted matrix

GitHub Actions run
[31072011794](https://github.com/js503/strukt/actions/runs/31072011794) validates
the implementation head on
[macOS 14](https://github.com/js503/strukt/actions/runs/31072011794/job/92521704067),
[Ubuntu 24.04](https://github.com/js503/strukt/actions/runs/31072011794/job/92521704089),
and
[Windows Server 2022](https://github.com/js503/strukt/actions/runs/31072011794/job/92521704129).
All three jobs passed, including strict format and lint, all-target tests, native
application builds, the M1 and M2 smoke paths, the M3 persistent-session smoke,
and the native Windows launch check. The completion documentation commit must pass
the same exact matrix before PR #12 is marked ready and merged.

The two preceding implementation runs passed macOS and Ubuntu and exposed two
Windows-only assumptions in sequence. First, fake clients supplied Unix-rooted
`/application-data` and helper paths; `88ccf6c` replaced them with platform-native
absolute fixtures while retaining strict production path validation. The next run
advanced through those tests and reached the native service, where LF-only test
input did not submit a ConPTY line. `f081736` aligned the session UI, smoke, and
native test with the terminal engine's portable carriage-return Enter framing and
added a direct regression. Focused native tests and strict lint pass locally.

## Accepted public-alpha limitations

- Iced's macOS accessibility bridge exposes the top-level window but not
  individually addressable controls or terminal cells. The walkthrough also did
  not show a visible Tab focus ring. Human accessibility verification remains an
  Alpha release gate.
- Real IME composition is not certified. Keyboard input, Unicode model behavior,
  and shortcut routing are automated, but human macOS and Windows IME checks
  remain Alpha gates.
- Human Windows visual QA remains an Alpha gate; M3 requires and runs hosted
  Windows service, ConPTY, native-launch, and session-smoke automation.
- Stable command IDs remain a provider/app boundary for the future command-palette
  surface; the current UI exposes the same actions as capability-gated controls.
- Session batch process jobs are bounded but serialized by the single-controller
  M3 service loop. M5 must use independently cancellable jobs before supporting
  multi-client or high-latency providers.

These limitations do not permit implicit process restart, workspace metadata,
network listeners, unauthenticated catalog access, or cloud dependence.
