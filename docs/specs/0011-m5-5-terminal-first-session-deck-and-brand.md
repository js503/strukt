# M5.5 — Terminal-First Session Deck and Brand Mark

Status: Approved for implementation planning

## Summary

This M5.5 refinement makes the terminal the default primary surface without
making it the only surface. The approved **Session Deck** composition presents
local or remote persistent sessions as the workspace's main operating model,
keeps files and developer tools immediately accessible, and allows the editor to
appear as a compact supporting drawer or promoted canvas.

The workspace bar replaces the visible `strukt` wordmark with the approved
**Bolt Structure** icon. The icon combines an isometric structural form with a
lightning path and uses one low-frequency, low-amplitude current animation.

This specification refines the visual and spatial contract in
[`0010-m5-5-visual-and-interaction-foundation.md`](0010-m5-5-visual-and-interaction-foundation.md).
It does not change terminal, editor, SSH, multiplexer, or session persistence
semantics.

## Approved Decisions

- Layout direction: **E — Session Deck**.
- Product posture: terminal-first, not terminal-only.
- Default primary surface: the active local or remote persistent terminal
  session.
- Supporting editor presentation: a promoteable lower drawer when the terminal
  owns the canvas.
- Brand direction: **A — Bolt Structure**.
- Brand presentation: icon only; the visible `strukt` word is removed from the
  workspace bar.
- Brand motion: one subtle current pass every 8.4 seconds with a long visually
  idle interval.
- Reduced motion: completely static final mark.

## Goals

1. Make persistent terminal sessions the clearest starting point for local and
   remote development.
2. Keep files, source control, diagnostics, editor, and future AI tools one
   keyboard action or stable navigation target away.
3. Make the workspace bar, activity rail, contextual sidebar, canvas, drawer,
   and status strip read as one contiguous system.
4. Give strukt a recognizable icon that communicates structure, lightning,
   speed, and impact without relying on a visible wordmark.
5. Keep persistent header animation calm enough for all-day use.

## Non-Goals

- Removing or demoting the editor as a first-class canvas surface.
- Reimplementing tmux, PTY persistence, SSH, or remote-helper behavior.
- Adding command blocks, AI features, new terminal semantics, or a new session
  protocol.
- Adding splash-screen, onboarding, marketing-site, or installer branding.
- Finalizing macOS, Windows, Linux, store, or package icon exports.
- Changing the approved activity-icon family.

## Session Deck Composition

### Unified workspace bar

The workspace bar spans the complete application width as one uninterrupted
surface. It contains, from left to right:

1. the Bolt Structure brand mark;
2. the execution boundary and workspace path;
3. the unified command control.

The brand mark occupies a 29-by-29-pixel visual box inside a 42-pixel identity
slot. No visible product name accompanies it. The mark exposes the accessible
name `strukt` and is decorative with respect to workspace state.

The workspace bar's lower hairline runs continuously across the window. Vertical
region dividers begin below that hairline and never overlap, double-stroke, or
project decorative pixels into the bar.

### Activity rail

The 48-pixel activity rail retains the approved icon-only navigation and
edge-to-edge square selection state. The rail is directly adjacent to the
session sidebar with exactly one shared divider.

Files remain a stable activity contribution even when Sessions owns the default
sidebar. Selecting Files replaces the contextual sidebar without replacing or
terminating the active terminal surface.

### Session sidebar

When Sessions is active, the contextual sidebar presents:

- local or remote execution-boundary status;
- the current host or local machine identity;
- persistent sessions;
- each session's window or pane count;
- attached, detached, stale, or reconnecting state; and
- stable entries for Files, Source Control, and Problems.

The active session receives one restrained semantic selection surface. Session
rows do not become cards and do not use large buttons.

### Primary terminal canvas

The active session owns the dominant canvas. Its header presents the active
session, window, pane, shell, and persistence state without repeating connection
details already visible in the workspace bar or sidebar.

Session-window tabs use the same contiguous tab geometry as editor tabs. Pane
splits use single hairline boundaries. Terminal content uses the native platform
monospace family and the shared typography scale.

### Promoteable editor drawer

When the terminal is primary, opening a file may place the editor in a compact
lower drawer. The drawer:

- shows the document path and modification state;
- preserves the open document and cursor state;
- can be closed without closing the document;
- can promote the editor to a split or full canvas; and
- can return the editor to its prior drawer position.

The existing promoteable-drawer state model remains authoritative. This visual
refinement changes composition, not lifecycle behavior.

### Status strip

The status strip reports the execution boundary, active session, repository
branch, connection latency when remote, and the active primary/supporting surface
relationship. It does not repeat static product identity.

## Typography and Divider Contract

- Application chrome uses the native system UI font.
- Terminal, editor, paths, identifiers, and fixed-width data use the native
  platform monospace font.
- Persistent chrome uses a bounded 11-to-14-pixel hierarchy: 14 pixels for the
  most important active labels, 12-to-13 pixels for normal controls and rows,
  and 11 pixels for metadata.
- Labels use medium weight at most. Large bold headings are prohibited in the
  persistent workspace shell.
- Every durable boundary is a single semantic hairline owned by exactly one
  adjoining region.
- Dividers terminate at shared grid intersections. Overlapping borders, inset
  shadows used as dividers, and doubled one-pixel lines are prohibited.

## Bolt Structure Brand Mark

### Geometry

The mark is a compact isometric structure with:

- one six-sided outer structural silhouette;
- two internal planes meeting at a central joint;
- one lightning path cutting vertically through the structure; and
- one small central energy node.

The geometry must remain legible at 20, 24, and 29 logical pixels. Stroke widths
scale from one canonical vector definition. The implementation may optimize
pixel alignment per platform but may not substitute a font glyph, emoji, or
unrelated small-size icon.

### Color

The structural silhouette uses the semantic primary text color. Internal planes
use the semantic muted text color. The lightning path and energy node use the
semantic accent color.

The mark must preserve the same hierarchy in built-in dark and light themes and
in future custom themes using the versioned semantic theme contract. Raw palette
values are prohibited.

### Motion

The loop duration is 8.4 seconds. Most of the cycle is visually idle. During one
brief pass:

1. a fine accent current traverses the existing lightning path;
2. the central energy node changes opacity and scale by a restrained amount; and
3. both return continuously to the identical resting frame.

The structure itself does not shake, rotate, translate, glow, blur, flash, or
change layout. Motion does not alter the logo's silhouette or legibility.

The animation pauses while the application window is unfocused or fully
occluded. The reduced-motion path renders the resting frame and schedules no
animation redraws.

### Accessibility

- The brand mark exposes `strukt` as its accessible name.
- The animation carries no status or required information.
- No workspace state relies on the mark's color or animation.
- The resting icon meets the same contrast requirements as persistent chrome.
- High-contrast themes may increase stroke contrast without changing geometry.

## State and Architecture Boundaries

The Session Deck is a composition policy over existing shell state. It does not
move session, terminal, editor, or remote lifecycle ownership into the view
layer.

The brand mark is a reusable `strukt-ui` vector component. Its renderer owns only
local animation progress and focus visibility. Theme colors and reduced-motion
policy are injected. The component emits no application messages and performs no
I/O.

The default terminal-first composition is a workspace preference boundary, not
a permanent architectural dependency. Future users or workspace templates may
choose another primary surface without duplicating the shell.

## Verification

Implementation is accepted only when all of the following pass:

1. layout contracts confirm one top workspace plane and single-owner dividers;
2. the Files activity remains reachable while Sessions is the default context;
3. terminal, editor drawer, promotion, and session behavior regressions pass;
4. icon geometry tests cover the canonical structural and lightning paths;
5. motion tests cover the 8.4-second cadence, idle interval, unfocused pause, and
   zero-redraw reduced-motion path;
6. dark, light, local, remote, wide, and compact native captures are reviewed;
7. the mark remains recognizable at 20, 24, and 29 logical pixels; and
8. the complete M1-through-M5 regression suite remains green.

## Acceptance Criteria

- A new workspace opens into the active persistent terminal session when one is
  available.
- A remote workspace clearly identifies its host, root, active session, and
  persistence state without duplicating labels across regions.
- Files are reachable without terminating or replacing the terminal process.
- Opening a file can show the editor drawer and promote it without losing state.
- The workspace bar reads as one continuous surface with clean divider
  intersections.
- Persistent chrome follows the 11-to-14-pixel typography hierarchy.
- The visible `strukt` wordmark is absent from the workspace bar.
- The Bolt Structure icon is the only visible brand identity in the workspace
  bar and exposes the accessible name `strukt`.
- Its loop is seamless, mostly idle, non-flashing, theme-aware, and completely
  static under reduced motion.
