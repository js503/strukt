# M5.5 Visual and Interaction Foundation Evidence

Status: In progress

## Deterministic verification

- `scripts/check-ui-semantics.sh` owns chrome-color enforcement.
- `scripts/m5-5-visual-foundation-smoke.sh` exercises both themes, every built-in activity, optional shell regions, terminal promotion/demotion, command search, and valid/invalid shell snapshots without user state, PTYs, workspace writes, or remote connections.

## Remote verification boundary

No disposable external SSH target was configured locally. The M4 fake-SSH smoke, M5 reconnect smoke, real helper integration tests, and CI disposable OpenSSH job cover the remote interaction boundary. No hostnames, usernames, addresses, keys, or terminal contents are committed here.

## Human walkthrough

The approved dark-theme macOS shell mockups were reviewed before implementation. A native macOS implementation screenshot confirmed the new hierarchy and exposed legacy native-widget accent coloring; the palette was then moved onto Quiet Precision tokens and guarded by a deterministic test. Windows and final dark/light responsive walkthroughs remain release-gate items.
