# M2.4 Language Intelligence Validation

- Status: Code validation complete; exact documentation head pending
- Date: 2026-08-02
- Issue: [#9](https://github.com/js503/strukt/issues/9)
- Pull request: [#10](https://github.com/js503/strukt/pull/10)
- Spec: [`../specs/0006-m2-language-intelligence.md`](../specs/0006-m2-language-intelligence.md)
- Plan: [`../plans/0006-m2-language-intelligence.md`](../plans/0006-m2-language-intelligence.md)

## Native contracts

The repository-owned language fixture is built as a real child process and
exercised over bounded stdio framing. The exact language smoke verifies discovery,
initialization, Unicode and CRLF synchronization, push diagnostics, completion,
hover, definition, cancellation observation, graceful shutdown, stopped
restoration, and absence of repository-local metadata.

Expected marker:

```text
strukt language smoke: discovery, sync, diagnostics, completion, hover, definition, cancellation, shutdown, and restore passed
```

The composed M2 smoke runs workspace/files, editor, native terminal, and language
contracts in one isolated workspace and requires this marker:

```text
strukt M2 integration smoke: files, editor, terminal, language, persistence, isolation, and stopped restore passed
```

## Local evidence

Verified on macOS from code head `9e58271` in the feature worktree:

- `forj check /Users/jessie/Development/strukt`: passed.
- `git diff --check` and `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --all-targets --locked --offline`: all tests passed,
  including 117 `strukt-app` tests and native language/terminal contracts.
- workspace-wide Clippy passed with warnings denied.
- `strukt-app`, `language-fixture`, and `terminal-fixture` native builds passed.
- `target/debug/strukt-app --language-smoke <temporary-root>`: exact marker observed.
- `target/debug/strukt-app --m2-integration-smoke <temporary-root>`: exact marker observed.
- Both smoke roots remained free of `.strukt` metadata.
- Linux `strukt-app` cross-check and Windows strict cross-target Clippy passed.

Only the documented future-incompatibility notice from transitive `block 0.1.6`
was emitted.

## Native macOS walkthrough

The native GPU application opened the isolated editor fixture and the real M2 Git
worktree. The walkthrough exercised the persistent file browser, editor and
terminal surfaces, context/Problems placement, theme switching, and stopped
restoration. It found a real linked-worktree defect: `.git` is a pointer file in a
Git worktree, but discovery attempted to read `.git/info/exclude` as a directory.
The regression now verifies that worktree roots open without the false warning.

The macOS inspection bridge continues to expose only the top-level Iced window,
not individual controls. It therefore cannot certify control-level accessibility,
focus order, IME composition, or visually inspect every fake-server overlay. The
repository-owned native smoke is the authoritative repeatable proof for visible
language data flow; human accessibility, IME, and Windows visual certification
remain release-gate limitations rather than claimed passes.

## Agentic review findings

The full-slice review covered trust, bounds, lifecycle ordering, generation and
revision guards, Unicode positions, Markdown sanitization, definition confinement,
persistence privacy, process cleanup, and M3/M4 boundary isolation. Material
findings and resolutions:

- server-initiated JSON-RPC requests were previously ignored; configuration
  requests are now answered and unsupported methods receive `Method not found`;
- failure details were not visible or copyable; bounded details and a copy action
  were added;
- linked Git worktrees produced a false `.git/info/exclude` warning; capability
  discovery now distinguishes a Git directory from a worktree pointer file;
- initialize, request, idle-shutdown, and shutdown deadlines existed in the domain
  client but were not polled by the app runtime; runtime-level red/green tests now
  enforce every deadline and stopped servers rediscover correctly;
- save notifications were unconditional; `didSave` now follows the server's
  advertised capability;
- Problems file grouping and persisted enable/disable plus ready-server restart
  controls were completed without overlapping process generations.

No unresolved critical or important code finding remains. The delivered spec
records intentional alpha scope: application crashes fail visibly and restart only
on explicit user action; external definitions are displayed but blocked; descriptor
selection and documentation links remain configuration-driven.

## Hosted matrix

Hosted validation exposed two test-fixture defects that local macOS execution could
not reproduce:

- run [30768978587](https://github.com/js503/strukt/actions/runs/30768978587)
  passed macOS 14 and Ubuntu 24.04 but failed Windows Server 2022 because two
  descriptor tests used Unix-only absolute fixture paths. Commit `d62a422` now
  constructs native absolute paths on both platform families.
- run [30769229439](https://github.com/js503/strukt/actions/runs/30769229439)
  passed macOS 14 and Ubuntu 24.04 but exposed a probabilistic recovery tamper test
  on Windows. The test assigned ciphertext byte zero to `1`, which made no mutation
  when the random encrypted byte was already `1`. Commit `907c034` now always
  changes the original byte.
- run [30769384837](https://github.com/js503/strukt/actions/runs/30769384837)
  passed macOS 14, Ubuntu 24.04, the full Windows app suite, and the corrected tamper
  test before four persisted-language tests exposed the remaining Unix-only
  `ResolvedCommand` fixture helper. Commit `5bb0cd2` gives that helper native
  absolute paths too. Its focused native suite and Windows-target strict Clippy
  pass, as does the full local workspace suite after the tamper fix.

Replacement run
[30769562813](https://github.com/js503/strukt/actions/runs/30769562813) is the
code-head matrix for `5bb0cd2`. It passed:

- [macOS 14](https://github.com/js503/strukt/actions/runs/30769562813/job/91554182376);
- [Ubuntu 24.04](https://github.com/js503/strukt/actions/runs/30769562813/job/91554182466);
- [Windows Server 2022](https://github.com/js503/strukt/actions/runs/30769562813/job/91554182365),
  including the full test suite, native application and fixture builds, and every
  deterministic M2 smoke.

The docs-only merge-ready head must pass the same matrix before merge.

## Security and privacy observations

- Workspace-provided server commands require an exact persisted fingerprint before spawn.
- Protocol headers, bodies, queues, completion items, hover content, diagnostics, and stderr are bounded.
- Diagnostic and definition paths are confined before automatic navigation.
- External and unsupported definition targets are displayed but never opened automatically.
- Restored language state contains selections, approvals, and Problems presentation only; no source, protocol payload, process identifier, or transient result is persisted.
