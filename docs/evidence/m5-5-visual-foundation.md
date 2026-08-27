# M5.5 Visual and Interaction Foundation Evidence

Status: In progress

## Deterministic verification

- `scripts/check-ui-semantics.sh` owns chrome-color enforcement.
- `scripts/m5-5-visual-foundation-smoke.sh` exercises both themes, every built-in activity, optional shell regions, terminal promotion/demotion, command search, and valid/invalid shell snapshots without user state, PTYs, workspace writes, or remote connections.

The local release gate on macOS passed:

- `cargo fmt --all -- --check`
- workspace Clippy with all targets, all features, locked offline dependencies, and warnings denied
- workspace tests across all targets
- workspace build across all targets
- the native M1 event-loop smoke and every deterministic M2–M5.5 smoke
- the semantic UI ownership guard

Retained regression suites cover large terminal output, editor input and recovery,
explorer refresh and persistence, diagnostics, session history, and SSH reconnect.
Terminal placement tests assert stable pane identity, working directory, process
count, and lifecycle across drawer, split, full canvas, and closed-view states.

## Performance

The optimized native startup smoke was run ten times after a release build. Wall
clock samples were `3.55`, `3.12`, `3.11`, `3.14`, `3.12`, `3.15`, `3.12`,
`3.14`, `3.10`, and `3.09` seconds; median: **3.12 seconds**. This smoke includes
the intentional three-second event-loop observation window, so it is a crash and
renderer-startup regression gate rather than a time-to-first-frame benchmark. No
separate comparable M1 wall-clock median was recorded; the same three-second M1
contract remains unchanged.

## Remote verification boundary

No disposable external SSH target was configured locally. The M4 fake-SSH smoke, M5 reconnect smoke, real helper integration tests, and CI disposable OpenSSH job cover the remote interaction boundary. No hostnames, usernames, addresses, keys, or terminal contents are committed here.

## Human walkthrough

The approved dark-theme macOS shell mockups were reviewed before implementation. A native macOS implementation screenshot confirmed the new hierarchy and exposed legacy native-widget accent coloring; the palette was then moved onto Quiet Precision tokens and guarded by a deterministic test. Windows and final dark/light responsive walkthroughs remain release-gate items.

## Agentic review

Overall risk: medium, driven by the size of the native-shell presentation migration
and cross-platform visual behavior. Review covered the approved spec and plan,
shell/theme/persistence boundaries, local and remote views, security-sensitive
remote state labels, focus and responsive contracts, and the complete diff.

Two material findings were identified and resolved before handoff:

- The command center described resource search but only returned commands. It now
  returns and activates matching files, recent workspaces, persistent sessions,
  saved remote workspaces, and commands while preserving execution boundaries.
- The drawer changed between Terminal and Problems implicitly. It now exposes
  explicit tool tabs and verifies that switching tools preserves terminal runtime
  identity.

No remaining material code findings were identified after the fixes and affected
tests. Remaining verification gaps are human Windows rendering/accessibility and
exact-head hosted CI; both are intentionally visible in milestone status.
