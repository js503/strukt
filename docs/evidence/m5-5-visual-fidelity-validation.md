# M5.5 Native Visual Fidelity Validation

Status: In progress — dark wide shell verified; editor, terminal, light, compact, split, and remote captures remain open.

## North-star

The acceptance reference is option A in
[`../mockups/visual-foundation/quiet-precision-spatial-policy.html`](../mockups/visual-foundation/quiet-precision-spatial-policy.html),
with the user-supplied editor and terminal-drawer composition treated as the
authoritative visual target.

## Native macOS capture

On 2026-09-04, the native app was launched from commit `09f4da0` on macOS at a
1,440 by 900 logical-pixel content size. A targeted capture of only the named
`strukt` window was reviewed; no full-screen or unrelated application content
was captured.

The capture confirmed:

- a 40-pixel workspace bar with a single-line 300-pixel command control;
- a 48-pixel icon-first activity rail with a 3-pixel accent selection indicator;
- a visible 218-pixel explorer at the default wide launch size;
- flat 27-pixel explorer rows and restrained semantic selection colors;
- edge-to-edge primary canvas composition without prototype outer padding;
- a flat supporting drawer and 25-pixel status strip;
- real drawer state in the status strip (`Problems open` rather than the incorrect
  `Terminal open` label);
- no bright default-widget button fills in the captured persistent chrome.

The first native capture also exposed and drove fixes for a wrapped command
control, a collapsed explorer caused by the default 1,024-pixel launch width,
boxed quiet controls, an accent-bordered empty-problems card, stale persisted
pane dimensions, and verbose status chrome.

## Remaining visual matrix

The milestone is not visually accepted until native captures cover:

- dark wide editor with two tabs and breadcrumbs;
- dark wide local terminal drawer;
- light wide editor and terminal drawer;
- compact responsive shell;
- drawer closed and drawer open;
- promoted split terminal;
- local and remote execution boundaries.

The current automation environment could identify and capture the `strukt`
window safely, but macOS denied synthetic keyboard input. It therefore could not
open the required editor and terminal reference state without user interaction.
This is a capture-state limitation, not an automated test failure.

## Automated verification

The correction slices have passed focused theme, UI, shell, persistence, app,
and visual-foundation contract tests. `scripts/check-ui-semantics.sh` now also
rejects raw default buttons from application-owned persistent chrome.

M5.5 remains **In review** until the remaining native matrix is captured and no
material delta from the north-star remains.
