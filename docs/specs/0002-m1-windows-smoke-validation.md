# M1 Windows Native Smoke Validation

- Status: Proposed
- Date: 2026-07-28
- Parent spec:
  [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)
- Implementation plan:
  [`../plans/0001-native-shell-foundation.md`](../plans/0001-native-shell-foundation.md)
- Tracking issue: [#1](https://github.com/js503/strukt/issues/1)
- Pull request: [#2](https://github.com/js503/strukt/pull/2)

## Summary

Milestone M1 must provide repeatable evidence that the native `strukt` executable
starts on Windows. The project is currently developed on an Apple silicon Mac
without access to a Windows computer or virtual machine.

The M1 application will therefore expose a deterministic `--smoke-test` launch
mode. It will initialize the real Iced application and GPU-rendered window, run the
event loop, report successful startup, and exit cleanly after a three-second
runtime-owned timer. GitHub Actions will execute this mode on its Windows runner.

This automated gate replaces M1's unavailable manual Windows window inspection. It
does not replace human Windows visual and interaction testing before the public
alpha.

## Goals

- Exercise the real Windows executable rather than only compiling it.
- Detect startup crashes, renderer initialization failures, nonzero exits, and
  event-loop hangs.
- Verify that the platform command modifier maps the shell shortcuts to Control on
  Windows.
- Keep normal interactive launches unchanged.
- Make the validation deterministic and suitable for every pull request.
- Record the difference between automated startup evidence and human visual QA.

## Non-goals

- Pixel-level or screenshot comparison on a hosted runner.
- Windows packaging, signing, installation, or auto-update validation.
- Accessibility, IME, or assistive-technology certification.
- Synthetic mouse or keyboard automation against rendered Iced widgets.
- Claiming that hosted startup validation replaces public-alpha Windows QA.

## Approaches Considered

### Self-terminating application mode

Add an explicit `--smoke-test` argument. The normal application boots with a
smoke-test launch mode, subscribes to a one-shot timer, reports success after the
event loop is active, and requests a clean Iced runtime exit.

This is the selected approach because it exercises the production startup path,
terminates deterministically, and avoids treating a force-killed process as a
successful application run.

### External process watchdog

Launch the normal executable, wait for it to remain alive, and terminate it from
PowerShell. This avoids application code changes, but it cannot distinguish a
healthy window from a process hung during initialization and never demonstrates a
clean shutdown.

### Manual Windows VM or remote desktop

Run the application interactively in a Windows VM or cloud desktop. This provides
the strongest visual evidence, but it introduces cost and maintenance that are not
justified for every M1 pull request. It remains part of public-alpha readiness.

## Design

### Launch mode

`strukt-app` will define a small launch-mode type with two values:

- `Interactive`, used when no smoke flag is present.
- `SmokeTest`, selected only by the exact `--smoke-test` argument.

Argument parsing will be isolated from the Iced view and domain crates. Unknown
arguments will not silently enable smoke behavior.

### Application lifecycle

Interactive mode preserves the current application behavior.

Smoke-test mode follows this sequence:

1. Parse `--smoke-test` before constructing the application.
2. Construct the same `StruktApp` state and native window used interactively.
3. Start the Iced event loop and renderer through the normal application entry
   point.
4. Subscribe to a one-shot three-second smoke timer in addition to keyboard events.
5. When the timer fires, print
   `strukt smoke test: native event loop started` and return `iced::exit()`.
6. Exit with status zero through the normal Iced runtime.

The GitHub Actions step will have a two-minute timeout so an event-loop hang
becomes a failed job rather than an indefinitely running workflow.

### Shortcut verification

Tests will construct real Iced keyboard events using `Modifiers::COMMAND`.
Iced defines that modifier as Command on macOS and Control on Windows and Linux.
The tests will cover:

- command+B toggles the explorer;
- command+J toggles the drawer;
- command+backslash toggles the context panel;
- the same keys without the platform command modifier do nothing.

Because the test suite runs natively in the Windows matrix job, it verifies the
Windows Control mapping from Iced's platform-specific implementation.

## CI

The existing matrix continues to run formatting, Clippy, tests, and the native
application build on macOS, Windows, and Linux.

The Windows job additionally runs:

```powershell
cargo run -p strukt-app -- --smoke-test
```

The step passes only when the application initializes, reaches the three-second
smoke timer, prints `strukt smoke test: native event loop started`, requests a
clean exit, and returns status zero. GitHub Actions terminates the step as a
failure after two minutes.

The smoke launch is Windows-only in M1 because macOS already has recorded manual
window validation and Linux remains an architectural/build target for the first
public alpha.

## Error Handling

- Renderer or window initialization errors propagate through `iced::Result` and
  fail the process.
- A smoke-mode timeout fails the Windows workflow.
- An unexpected nonzero exit fails the Windows workflow.
- Interactive launches never schedule the smoke timer.
- Unknown arguments cannot shorten an interactive session.

## Evidence and Governance

After the hosted Windows smoke gate passes:

- `docs/evidence/m1-native-shell-validation.md` records the workflow run and the
  limits of automated evidence.
- ADR 0001 may accept Iced for the M1 foundation while explicitly deferring human
  Windows visual QA to M9.
- The M1 issue and PR record the Windows smoke result.
- The roadmap and tracker may mark M1 complete only when every other M1 exit
  criterion is also satisfied.

M9 remains blocked from completion until a human validates the packaged Windows
application on a real or virtual Windows desktop.

## Acceptance Criteria

1. Normal launches remain interactive and do not auto-exit.
2. `--smoke-test` uses the real native application startup path.
3. Smoke mode exits cleanly after three seconds with
   `strukt smoke test: native event loop started` after the Iced event loop starts.
4. Windows CI fails on startup error, nonzero exit, or timeout.
5. Windows-native tests verify Control-based explorer, drawer, and context
   shortcuts through `Modifiers::COMMAND`.
6. Validation evidence states that hosted startup coverage is not human visual QA.
7. Public-alpha documentation retains mandatory human Windows validation.
