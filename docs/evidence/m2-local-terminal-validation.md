# M2 Local Terminal Validation

- Date opened: 2026-07-31
- Branch: `feat/m2-terminal`
- Pull request: [#8](https://github.com/js503/strukt/pull/8)
- Tracking issue: [#7](https://github.com/js503/strukt/issues/7)
- Local platform: macOS on Apple silicon
- Rust: 1.97.1
- Scope: M2.3 local ephemeral PTY/ConPTY terminals

Implementation head `cc91b80` completed local review, the native bundled-app
walkthrough, and the hosted macOS, Ubuntu, and Windows validation matrix.

## Deterministic Terminal Smoke Contract

The headless `--terminal-smoke <existing-root>` workflow uses the separately built
`terminal-fixture` executable rather than the user's shell. It verifies native PTY
spawn, Unicode input/output, ANSI cell state, resize propagation into a renderer
snapshot, independent process exit, a live quiet pane during load, 64 MiB of
bounded scheduler progress, per-pane and aggregate drain budgets, graceful
termination, a nested three-pane split, stopped-only restoration, and absence of
workspace-local `.strukt` metadata. The native adapter load is 64 MiB on Unix and
1 MiB on Windows; the separate 64 MiB runtime stress executes on every hosted OS.

On success it exits zero and prints exactly:

```text
strukt terminal smoke: pty, unicode, ansi, resize, isolation, bounds, and restore passed
```

Launch-mode tests reject missing roots, nonexistent roots, extra arguments, and
near-match flags. The smoke has an internal 30-second deadline and the CI step has
a two-minute outer timeout.

## Local Verification

| Command | Result |
|---|---|
| `forj check /Users/jessie/Development/strukt` | Pass; governed primary checkout is valid |
| `git diff --check` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --all-targets --locked --offline` | Pass; 287 test executions, 0 failures |
| `cargo build -p strukt-app --locked --offline` | Pass |
| `cargo build -p strukt-terminal --bin terminal-fixture --locked --offline` | Pass |
| `target/debug/strukt-app --terminal-smoke <temporary-fixture>` | Pass in about four seconds; exact marker emitted |
| `cargo check -p strukt-app --target x86_64-unknown-linux-gnu --locked` | Pass |
| `cargo clippy -p strukt-terminal -p strukt-persistence -p strukt-workspace -p strukt-app --target x86_64-pc-windows-msvc --all-targets --locked --offline -- -D warnings` | Pass |

Only the previously documented transitive `block 0.1.6` future-compatibility
warning remains.

## Hosted CI Contract

The macOS 14, Ubuntu 24.04, and Windows Server 2022 jobs build the deterministic
fixture, create an isolated existing directory, run the exact smoke command,
require the exact marker, and reject `.strukt` metadata. Windows therefore runs
the native ConPTY contract rather than relying on cross-compilation alone.

Implementation-head run: [30728609554](https://github.com/js503/strukt/actions/runs/30728609554).

- [macOS 14 job](https://github.com/js503/strukt/actions/runs/30728609554/job/91444741734) — pass in 1m 0s
- [Ubuntu 24.04 job](https://github.com/js503/strukt/actions/runs/30728609554/job/91444741740) — pass in 57s
- [Windows Server 2022 job](https://github.com/js503/strukt/actions/runs/30728609554/job/91444741702) — pass in 3m 55s

Diagnostic run [30725593231](https://github.com/js503/strukt/actions/runs/30725593231)
passed macOS and Ubuntu but exposed a Windows ConPTY cursor-inheritance deadlock.
The job was cancelled after the log isolated all three native contract cases at the
startup handshake. Follow-up diagnostics ruled out test concurrency and showed the
responder must remain armed after child creation. Final head `26837bd` starts the
output reader before child creation and keeps a one-shot bounded responder armed
until ConPTY emits its cursor-position query. This follows
Microsoft's [`CreatePseudoConsole` contract](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole),
which requires an asynchronous response when cursor inheritance is enabled.
Run [30727227748](https://github.com/js503/strukt/actions/runs/30727227748)
then passed the native ConPTY contract and exposed a separate smoke-only input
assumption: LF submitted the fixture line on Unix but not through ConPTY. Head
`1870624` frames the deterministic line as CRLF, matching the shared transport
contract and the platform Enter boundary.

Run [30727855023](https://github.com/js503/strukt/actions/runs/30727855023)
then showed that resizing ConPTY before visible output could erase the smoke's
evidence. Head `0acef4c` waits for Unicode and ANSI observations, resizes the live
fixture, and sends an explicit completion line. Runs
[30728177306](https://github.com/js503/strukt/actions/runs/30728177306) and
[30728341581](https://github.com/js503/strukt/actions/runs/30728341581) exposed the
last portability boundary: ConPTY coalesces repeated cursor controls and renders
large visible console streams rather than forwarding bytes transparently. Final
head `cc91b80` therefore keeps the 64 MiB scheduler stress identical on every OS,
uses a bounded 1 MiB native ConPTY load, and retains the 64 MiB native Unix load.

## Responsiveness Stress

The deterministic scheduler test advances 1,024 chunks of 64 KiB for exactly
64 MiB while a quiet pane produces and retains `quiet-progress`. It completed in
1.28 seconds in the final local debug test run and passes in all three hosted jobs. Each
drain remains capped at 256 KiB per pane and 1 MiB aggregate; the native transport
queue remains capped at 4 MiB or 1,024 chunks. Separate pressure-state regression
coverage confirms backpressure becomes visible and clears after recovery. The app
reducer's file, editor, persistence, and terminal action tests remain green in the
same complete workspace suite.

## Native macOS Walkthrough

A temporary native `.app` bundle was built from the implementation head and opened
against `/private/tmp/strukt-m2-editor-manual`. The walkthrough verified:

- a restored terminal is visibly stopped and does not start until `Start / restart`;
- the default local zsh starts in the workspace root and renders a live cursor;
- keyboard input reaches the PTY, Unicode `λ`, ANSI red text, wrapping, and a
  detected `https://example.com` target render on the GPU terminal surface;
- a horizontal split starts a second independent live zsh in the same workspace;
- expanding moves both terminal panes into the primary workspace canvas without
  restarting their processes or losing output;
- the workspace fixture remains free of `.strukt` metadata.

Selection extraction, scrollback-aware copy, native clipboard error handling,
bracketed and large-paste consent, exact-target link opening, resize, tabs, nested
splits, focus and mouse reporting, rename focus isolation, exit/restart, close
confirmation, light/dark semantic tokens, sustained output, restoration, and
keyboard traversal are also covered by deterministic reducer, widget, runtime, and
smoke tests. The native smoke keeps a quiet pane responsive during bounded output;
the separate scheduler test drives the cross-platform 64 MiB responsiveness gate.

The automation bridge exposes the native window but no individually addressable
Iced controls or terminal cells. It can click the rendered controls by coordinate,
but this is not an accessibility certification. Unicode input was verified;
composition through a real macOS IME was not independently observable through the
bridge. These remain explicit framework risks for continued validation and M9
human accessibility/IME certification rather than being overstated as passes.

## Review Status

The full-slice review covered PTY ownership and cleanup, ConPTY portability,
process termination, bounded queue accounting, deadlock and fairness risks, stale
generations, escape/OSC limits, Unicode and wide-cell invariants, scrollback-aware
selection, link and paste safety, persistence privacy, accidental restart,
custom-widget routing, rendering bounds, capability disablement, and M3/M4 scope.

Resolved findings include:

- moved blocking spawn, restart termination, and close termination out of the Iced
  update reducer;
- rejected stale async spawn completions by pane generation;
- surfaced sustained native-reader pressure instead of observing only reducer
  backlog;
- copied selections from their actual scrollback viewport;
- added focus and mouse reporting, selection painting, terminal-tab/pane keyboard
  traversal, and primary-canvas expansion;
- made running-pane restart explicit and kept close confirmation for live panes;
- preferred PowerShell 7/Windows PowerShell before `cmd.exe` on Windows;
- prevented ConPTY startup deadlock by draining before child creation and keeping
  a one-shot responder armed until the cursor-inheritance query arrives;
- made the deterministic smoke submit line input with portable CRLF framing;
- made link opening a separate scheme-allowlisted action with the exact target;
- prevented tab-name edits from leaking into the shell and made Ctrl+Tab win over
  platform-command handling on Windows-style modifier combinations.

Every critical or important code finding was resolved with focused regression
coverage. Remaining limitations are the explicitly recorded Iced accessibility/IME
inspection gap and lack of human Windows visual certification, which is an M9 gate.
