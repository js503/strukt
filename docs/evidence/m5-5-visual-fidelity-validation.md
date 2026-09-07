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

## Activity rail icon refinement

On 2026-09-04, the approved activity-rail refinement replaced the temporary
Unicode glyphs and the former three-pixel side indicator with a native vector
family. Every icon now owns one normalized geometry definition that is reused
for its resting outline and selected rendering. The selected frame occupies the
full 48-by-48-pixel rail width with square corners and no accent bar.

Selection animates over 180 milliseconds by projecting the same geometry into a
restrained top-down 2.5D treatment: two crisp depth passes, the canonical face,
and one fine accent highlight. The renderer does not use glow, blur, soft drop
shadows, or alternate selected glyphs. Its reduced-motion path resolves directly
to the final frame.

The implementation was developed red-green: the new vector-family and rail
geometry contracts first failed because the geometry API and constants did not
exist, then passed after the renderer and rail integration were added.

Fresh verification from the refinement working tree:

- `cargo test -p strukt-ui --test component_contracts`: 6 passed;
- `cargo test -p strukt-app --test visual_foundation_contract`: 3 passed;
- `cargo fmt --all --check`: passed;
- `cargo clippy --workspace --all-targets -- -D warnings`: passed;
- `bash scripts/check-ui-semantics.sh`: passed;
- `bash scripts/m5-5-visual-foundation-smoke.sh`: passed with the expected M5.5
  completion marker;
- `cargo test --workspace -q`: passed, including 150 core tests and all workspace
  integration-test binaries; one pre-existing ignored test remained ignored;
- `forj check .`: passed against the repository manifest;
- `git diff --check`: passed.

The native app launched successfully with `cargo run -p strukt-app`. Automated
window inspection could not attach to the raw development executable, so the
icon refinement's final human visual review remains part of the open native
matrix above. This does not change the automated result, and M5.5 remains
**In review** rather than being marked complete.

## Session Deck and Bolt Structure refinement

On 2026-09-04, the approved terminal-first Session Deck and icon-only Bolt
Structure identity were implemented from
[`../specs/0011-m5-5-terminal-first-session-deck-and-brand.md`](../specs/0011-m5-5-terminal-first-session-deck-and-brand.md)
using
[`../plans/0013-m5-5-session-deck-and-brand.md`](../plans/0013-m5-5-session-deck-and-brand.md).

The shell now uses Sessions as the primary canvas for new workspaces while
preserving valid compositions saved by earlier schema versions. Files remains a
stable contextual sidebar and does not replace the session canvas. Explicitly
opening a document places the editor in a 148-pixel supporting drawer with
split, full, return-to-drawer, and close controls; restoring a document preserves
the saved supporting-surface placement.

The workspace bar now contains only the 29-pixel Bolt Structure mark in a
42-pixel identity slot; the visible workspace-bar wordmark is gone. The mark is
drawn from canonical structural, plane, bolt, and energy-node geometry using
semantic theme colors. Its pure motion sampler and canvas scheduler implement an
8.4-second mostly-idle cycle, one restrained current pass, focus pause, and a
zero-redraw reduced-motion path.

Native macOS review used an app-scoped capture of only the named `strukt` window.
The durable capture is
[`m5-5-session-deck-macos.png`](m5-5-session-deck-macos.png). It confirms the
continuous workspace bar, icon-only brand, 48-pixel rail, Sessions boundary
sidebar with stable Files, Source Control, and Problems entries, dominant
terminal surface, compact 11-to-13-pixel metadata, restored editor peek,
active-session and branch status, and single-owner horizontal and vertical
dividers at the 1,440-by-900 logical-pixel launch size.

The final review also closed the interaction defects found by agentic review:
editor promotion no longer reuses terminal messages or activates hidden PTY
input, pre-v4 snapshots no longer discard an explicit composition, reduced
motion is persisted and drives both brand and activity-icon animation, and
remote connection setup records handshake latency for the status strip. Iced
The Settings canvas now exposes explicit Normal motion and Reduced motion
controls, and snapshots created before the preference existed migrate to the
safe reduced-motion state. Local workspaces discover their Git branch during
the existing blocking workspace-open task; the refreshed capture shows
`feat/m2-language` rather than an unknown placeholder.

Iced
does not currently expose a native accessibility-name setter for its canvas
buttons, so icon-only controls retain visible hover tooltips and keyboard paths,
but native screen-reader naming remains an explicit unmet alpha accessibility
gate. Full-window occlusion is likewise not exposed to the canvas program;
unfocused windows pause animation and reduced motion schedules no redraws.

Fresh verification after the final cleanup:

- `cargo fmt --all --check`: passed;
- `cargo clippy --workspace --all-targets -- -D warnings`: passed;
- `cargo test --workspace`: passed, including 154 `strukt-app` tests and every
  workspace integration-test binary; the explicitly configured real-OpenSSH
  test remains ignored;
- `bash scripts/check-ui-semantics.sh`: passed;
- `bash scripts/m5-5-visual-foundation-smoke.sh`: passed with the expected M5.5
  completion marker;
- `forj check .`: passed;
- `git diff --check`: passed.

The native app remains running from `cargo run -p strukt-app` for interactive
review. Light-theme, remote-host, active multi-session, and live editor
promotion captures remain part of the milestone-wide visual matrix, so M5.5
correctly remains **In review**.
# Header gutter correction — 2026-09-05

Follow-up: search now uses a borderless, vertically centered header control with
hover/pressed feedback. The activity rail no longer paints a duplicate border or
top inset. Explorer reserves one pixel inside its panel border so file rows do
not overpaint it. Both added source regression guards failed before the change
and passed afterward. UI tests (8), app visual contracts (5), formatting,
semantic ownership, and focused app/UI clippy passed. The final build launched.
[Native search capture](m5-5-integrated-search-macos.png) verifies header alignment
and the Sessions rail. Populated Explorer visual verification remains pending:
computer-use could not resolve the unbundled development executable, so the
capture does not prove the file-row artifact is eliminated. These guards test
source structure, not rendered pixels.

The workspace bar painted only its shrink-height child, leaving a blank strip
inside the fixed-height parent and inset side gutters. The painted container now
owns the full 40-pixel height and width; content padding is internal and vertically
centered. No session or editor lifecycle behavior changed.

Verification: the source regression guard failed before the fix, then all four
`visual_foundation_contract` tests passed. `cargo fmt --all --check` and
`bash scripts/check-ui-semantics.sh` passed. Launched `cargo run -p strukt-app`
and inspected the native window capture: no blank horizontal strip beneath the
workspace bar or inset header surface. The source guard is not a pixel-layout
test; native evidence is [the rebuilt window](m5-5-header-gap-fixed-macos.png).
The complete cross-platform visual acceptance matrix remains open.
# Quiet Session Deck — first implementation pass

Approved spec 0012 and plan 0014 govern this pass. Application text defaults to
13px; shared labels are explicitly sized and centered; local/remote editors use
13px monospace. Supporting editor composition combines tabs, action menus, and
placement controls into one row. Session tools are opt-in; existing management
and input forms, capability guards, errors, and confirmations are retained.
Session navigation highlights only the selected leaf. Operation errors replace
the otherwise-successful status label without changing provider lifecycle state.

Verification: 155 app unit tests, six app visual-contract tests, eight UI tests
passed. Focused app/UI clippy with warnings denied, formatting, semantic guard,
and diff whitespace checks passed. A native capture verified the consolidated
drawer and populated Explorer boundary: [first pass](m5-5-quiet-deck-first-pass.png).
The capture precedes the final shared-label centering and status grouping tweak.

Still open: interactive compact-window/tab overflow review, direct persistent
terminal input quality, provider-unavailable reproduction and recovery, native
font-family resolution, light/remote captures, and subtle panel transitions with
reduced motion. Source guards are not substitutes for pixel or interaction tests.
No claim of a bug-free interface or completed M5.5 acceptance is made.
