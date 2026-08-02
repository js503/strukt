# M2.4 Language Intelligence Validation

- Status: Local validation complete; hosted matrix pending final PR head
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

Verified on macOS from the feature worktree:

- `cargo test --workspace --all-targets --locked --offline`: all tests passed,
  including 112 `strukt-app` tests and native language/terminal contracts.
- strict `strukt-app` Clippy: passed with warnings denied and pedantic lints enabled.
- `target/debug/strukt-app --language-smoke <temporary-root>`: exact marker observed.
- `target/debug/strukt-app --m2-integration-smoke <temporary-root>`: exact marker observed.
- Both smoke roots remained free of `.strukt` metadata.
- Linux `strukt-app` cross-check and Windows strict cross-target Clippy passed.

The final full-workspace gate and hosted macOS, Ubuntu, and Windows results are
recorded against the exact merge-ready head during Task 11.

## Security and privacy observations

- Workspace-provided server commands require an exact persisted fingerprint before spawn.
- Protocol headers, bodies, queues, completion items, hover content, diagnostics, and stderr are bounded.
- Diagnostic and definition paths are confined before automatic navigation.
- External and unsupported definition targets are displayed but never opened automatically.
- Restored language state contains selections, approvals, and Problems presentation only; no source, protocol payload, process identifier, or transient result is persisted.
