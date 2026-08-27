# M5.5 — Visual and Interaction Foundation

Status: Approved

## Summary

M5.5 redesigns the existing `strukt` interface around the approved **Quiet
Precision** direction before the Public Alpha release. It replaces the current
prototype presentation with a coherent visual system, adaptive workspace shell,
reusable native UI components, dual built-in themes, and an extensible semantic
theme contract.

This milestone may reorganize navigation and existing workflows, but it does not
add new product capabilities. Every M1 through M5 capability remains available and
must pass regression verification after its surface is migrated.

## Motivation

The current application proves the required native architecture and workflows, but
its presentation is still prototype-quality. Feature views compose mostly default
Iced widgets with raw text labels, large accent-colored buttons, inconsistent
spacing, weak hierarchy, and permanently visible panels. The adaptive workspace
model described by the product foundation is present conceptually but is not yet
expressed by the interface.

Public Alpha requires an interface that external developers can understand and use
without interpreting implementation scaffolding. The visual foundation must land
before packaging so the release walkthrough, screenshots, documentation, and
accessibility verification evaluate the intended product rather than a temporary
shell.

## Approved Product Decisions

- Milestone identifier: **M5.5 — Visual and Interaction Foundation**.
- Design direction: **Quiet Precision**.
- Workspace model: **adaptive canvas**; no single tool is permanently central.
- Scope: full UX redesign of existing M1 through M5 surfaces.
- Themes: dark and light are equally supported built-ins.
- Future themes: built-ins use the same versioned semantic contract reserved for
  custom themes.
- Secondary tools: open in a **promoteable bottom drawer** and may be promoted to a
  split or full canvas surface.
- New product capabilities remain outside M5.5.

## Goals

1. Make the native application visually coherent, restrained, legible, and ready
   for external evaluation.
2. Express the Focus + Context product model through one task-owned adaptive
   canvas, stable global navigation, and on-demand supporting surfaces.
3. Replace feature-owned presentation choices with reusable components and
   semantic visual roles.
4. Preserve modular capability boundaries so disabling a feature removes its UI
   contributions without leaving broken navigation or layout.
5. Ship equivalent, polished dark and light themes.
6. Establish a validated, versioned theme-definition boundary that future custom
   theme loading can reuse.
7. Preserve keyboard-first operation, accessibility semantics, native performance,
   and every verified M1 through M5 behavior.

## Non-Goals

M5.5 does not implement:

- AI providers, conversations, context collection, agents, or AI tools;
- plugin loading, MCP discovery, a marketplace, or external UI contributions;
- external custom-theme discovery, installation, synchronization, or distribution;
- new file, editor, terminal, language, session, SSH, or remote-helper features;
- packaging, signing, notarization, installers, telemetry, release publication, or
  Alpha user documentation;
- a web UI, Electron shell, or non-native rendering path.

The Public Alpha gate follows M5.5. M6 and later product capabilities remain
post-Alpha.

## Design References

- Approved local and remote workflow mockup:
  [`../mockups/visual-foundation/quiet-precision-workflows.html`](../mockups/visual-foundation/quiet-precision-workflows.html)
- Approved secondary-tool comparison and promoteable-drawer decision:
  [`../mockups/visual-foundation/quiet-precision-spatial-policy.html`](../mockups/visual-foundation/quiet-precision-spatial-policy.html)
- Foundational Focus + Context direction:
  [`../mockups/workspace-shell/focus-context.html`](../mockups/workspace-shell/focus-context.html)
- Product and remote-development foundation:
  [`0001-workspace-shell-and-remote-development.md`](0001-workspace-shell-and-remote-development.md)

The mockups define hierarchy, density, and interaction intent. They are not a
pixel-perfect substitute for native platform review.

## Quiet Precision Visual Language

Quiet Precision is compact, calm, and tool-first. The active work receives the
strongest contrast. Navigation and supporting information remain visible without
competing for attention.

### Geometry and spacing

- Components use square or nearly square geometry with a two-to-four-pixel corner
  radius.
- Layout follows a compact four-pixel spacing rhythm.
- Hairline boundaries separate durable regions. Filled cards are reserved for
  contained states, not used as the default layout primitive.
- Dense lists retain a usable pointer target and an explicit keyboard focus target;
  visual density cannot reduce accessibility bounds below the documented platform
  minimum.
- Shadows are limited to temporary elevation such as command palettes, menus, and
  focused confirmations.

### Typography and icons

- Native system UI fonts render application chrome and prose.
- The native platform monospace font renders editor, terminal, identifiers, and
  fixed-width data.
- A single internally licensed vector icon family supplies navigation and action
  symbols. Icons do not mix emoji, arbitrary Unicode approximations, and unrelated
  visual families.
- Icon-only controls require an accessible name, tooltip, focus treatment, and
  keyboard path.
- Destructive or security-sensitive actions retain explicit text labels.

### Color and emphasis

- Neutral surfaces provide the hierarchy; the accent does not fill every action.
- Accent color identifies selection, active focus, primary progress, and a small
  number of important actions.
- Success, warning, error, remote, stale, unread, attention, and destructive states
  use semantic roles.
- No state relies on color alone. Iconography and text accompany semantic color.
- Dark and light themes preserve the same hierarchy even when their exact surface
  relationships differ.

### Motion

- State transitions use restrained 120–180 millisecond motion.
- Motion communicates spatial change, focus, drawer promotion, or state
  replacement; it is not decorative.
- Reduced-motion preferences eliminate nonessential transitions while preserving
  final state and focus movement.

## Workspace Composition

The workspace shell has six compositional regions.

### 1. Workspace bar

The top workspace bar displays:

- product and workspace identity;
- local or remote execution boundary;
- remote alias and root when applicable;
- connection or stale state when applicable; and
- entry to the unified command center.

The bar does not duplicate common feature actions. Feature actions live with the
surface they affect or in the command center.

### 2. Activity rail

The stable activity rail contains contributions for Files, Search, Source Control,
Sessions, Connections, Extensions, and Settings when their capabilities are
available. The active item uses a restrained indicator instead of a fully filled
button.

An unavailable capability removes or explicitly disables its contribution
according to the capability contract. The shell never leaves an empty placeholder.

### 3. Contextual sidebar

The sidebar follows the selected activity. It contains navigation and controls for
that activity, such as the file tree or session hierarchy. It is collapsible,
resizable within bounded limits, and restored per workspace.

The sidebar does not host global commands or duplicate content from the context
panel.

### 4. Adaptive canvas

The active task owns the central canvas. Existing canvas surface types include:

- editor documents;
- local and remote terminals;
- local and remote session workspaces;
- source-control views;
- search results;
- connection management; and
- settings.

A promoted drawer tool becomes a canvas surface without restarting its underlying
operation or losing its view state.

### 5. Promoteable drawer

Terminal, Problems, logs, tasks, and diagnostics open in a bounded bottom drawer by
default when another surface owns the canvas. The drawer:

- supports tabs for multiple tool types;
- remembers a bounded height per workspace;
- can collapse without terminating processes;
- can promote its active tool to a split or full canvas surface;
- can return a promoted tool to its prior drawer position; and
- preserves feature-owned process and selection state through composition changes.

Drawer presentation state may persist. Terminal output, command content,
environment data, clipboard content, and other data prohibited by existing specs
remain non-persistent.

### 6. On-demand context panel and status strip

The right context panel is hidden by default. It opens for information that is
relevant to the active task, including diagnostics, connection details, language
status, approvals, or future AI context. Closing it does not cancel the underlying
operation.

The compact status strip communicates durable boundary and health information such
as local versus remote execution, branch, diagnostics count, session state,
language mode, cursor location, and connection latency. It is not an overflow menu
for arbitrary actions.

## Unified Command Center

`CommandOrControl+K` opens one command center over the current workspace. It can
search and activate:

- files and recent files;
- commands and exact actions;
- local and remote workspaces;
- local and remote persistent sessions;
- settings;
- activities and open surfaces; and
- feature-provided actions allowed by the capability registry.

Results identify their type, execution boundary, keyboard shortcut, disabled
reason, and destructive or approval requirement. Search is incremental and
bounded. Opening the command center never starts a process, connects to a host, or
executes a command without the existing explicit action path.

Escape returns focus to the previously focused control. Executing an action moves
focus to the resulting surface or confirmation.

## Component and Module Architecture

### `strukt-theme`

`strukt-theme` remains UI-framework independent and expands to own:

- `ThemeId` and display metadata;
- `ThemeDefinitionV1` with dark and light variants;
- primitive palette values;
- semantic application, editor, terminal, syntax, diagnostic, connection, and
  session roles;
- component-independent metrics that themes are allowed to customize;
- definition validation and contrast diagnostics;
- resolution into complete runtime `ThemeTokens`; and
- safe fallback to a built-in definition.

Built-in Quiet Precision themes are expressed through `ThemeDefinitionV1`, not a
separate hard-coded path. The versioned definition is serializable and suitable for
future external theme loading, but M5.5 exposes no external loader.

Themes may customize color and a bounded set of visual metrics. They cannot alter
layout ownership, remove focus visibility, hide security boundaries, change
minimum target sizes, or inject executable behavior.

### `strukt-ui`

A new Iced-dependent `strukt-ui` crate owns reusable native presentation
primitives and maps resolved semantic tokens to Iced styles. Initial primitives
include:

- application and workspace chrome;
- icon, quiet, primary, toggle, and destructive buttons;
- activity-rail items;
- toolbars and command inputs;
- tabs and tab strips;
- tree and list rows;
- panel, sidebar, drawer, split, and status containers;
- badges and health indicators;
- fields, pickers, menus, tooltips, and popovers;
- empty, loading, unavailable, stale, and error states;
- confirmations and approval surfaces; and
- focus and selection indicators.

`strukt-ui` contains no workspace, file, process, SSH, session, or provider logic.

### `strukt-shell`

`strukt-shell` remains UI independent and owns compositional state:

- active activity and canvas surface;
- available contributions;
- sidebar visibility and bounded width;
- drawer visibility, active tool, bounded height, and promotion target;
- split topology and ratios;
- context-panel visibility;
- focused region and focus-return history; and
- reduced-motion and density preferences relevant to shell behavior.

State transitions are typed and deterministic. A feature cannot mutate unrelated
shell regions directly.

### Feature contributions

Each enabled feature provides typed contributions for the activities, commands,
canvas surfaces, drawer tools, context sections, and status items it owns. The
shell orders contributions through stable host policy rather than feature load
order.

Removing a capability removes its contributions, closes presentation surfaces with
the existing confirmation policy, and preserves unrelated workspace state.

Feature views compose `strukt-ui` primitives and semantic roles. A feature may use
domain-specific layout inside its surface, but it cannot define raw application
chrome colors or global spacing rules.

## Theme Data Flow and Failure Handling

Theme selection follows this pipeline:

1. Persistence restores a selected `ThemeId` and mode preference.
2. The theme registry resolves the versioned definition.
3. Validation checks completeness, value bounds, protected roles, and required
   contrast relationships.
4. The resolver produces complete immutable semantic tokens.
5. `strukt-ui` maps tokens to component states.
6. Feature views render reusable primitives with no raw palette access.

An unknown, incompatible, or invalid theme never partially applies. The resolver
uses the matching built-in Quiet Precision mode, records a local diagnostic, and
shows a recoverable Settings notice identifying the rejected definition and
reason. The fallback does not block workspace startup.

Theme switching is atomic for one application frame. It does not restart terminal,
language, session, or remote processes.

## State Language

Every surface uses the same state vocabulary.

### Empty

Explain the purpose of the surface and offer one primary next action. Do not fill
empty space with disabled controls.

### Loading

Preserve stable layout and show bounded progress near the affected content.
Unrelated tools remain interactive. Indeterminate progress never implies a known
completion percentage.

### Unavailable

Name the missing capability or platform limitation and provide the available
repair, configuration, or fallback action. Do not present unavailable behavior as
an empty success state.

### Stale or disconnected

Retain the last valid content when existing security and privacy rules permit it,
mark the content non-live, identify the execution boundary, and expose reconnect
or refresh. Stale remote content cannot claim that a save, process, diagnostic, or
session state is current.

### Recoverable error

Show an inline explanation, Retry when safe, and bounded diagnostics. Repeated
failure does not create stacked dialogs or unbounded notifications.

### Destructive or security-sensitive decision

Use a focused confirmation surface with explicit target, boundary, consequence,
and cancel path. Existing exact-command, helper-installation, clipboard, link,
file-deletion, process-termination, and remote-host policies remain authoritative.

## Keyboard, Focus, and Accessibility

- Every pointer action has a keyboard path unless the platform API itself lacks
  one and the limitation is documented.
- Focus order follows visible spatial order and skips hidden or disabled regions.
- Collapsing, promoting, closing, or replacing a region returns focus to the most
  recent valid control in the prior region.
- Focus is always visually distinct from selection and hover.
- Controls expose names, roles, values, state, and relationships through available
  native accessibility APIs.
- Text and meaningful interface content support platform scaling without clipping
  critical actions.
- Semantic status never depends on color, animation, or iconography alone.
- Reduced motion is honored throughout shell and component transitions.
- Keyboard conventions use Command on macOS and Control on Windows and Linux while
  preserving platform-reserved shortcuts.

## Responsive Density and Platform Behavior

Quiet Precision is dense but not fixed to one screenshot size.

- The adaptive canvas receives width before optional supporting regions.
- The context panel closes before the contextual sidebar when width is constrained.
- The sidebar can collapse to the activity rail while retaining an explicit reopen
  action.
- The drawer height is bounded relative to the window and never reduces the canvas
  below its minimum usable height.
- Split ratios clamp to usable bounds and collapse through an explicit policy when
  the window cannot support both surfaces.
- Native title-bar accommodation, window controls, font metrics, shortcuts, and
  accessibility behavior are validated separately on macOS and Windows.
- Linux follows the shared layout and theme contracts and remains part of hosted
  build and automated rendering verification.

## Persistence and Privacy

The shell persists only presentation state already allowed by the workspace
snapshot model or explicitly added by this spec:

- selected theme and light/dark mode;
- active activity and surface identifiers;
- sidebar, drawer, context-panel, and split presentation state;
- bounded region sizes; and
- focus-return identifiers that contain no content.

Persistence rejects invalid dimensions, unknown surface identifiers, impossible
split topology, unavailable capabilities, and incompatible schema versions through
safe defaults.

The redesign does not expand persisted terminal output, commands, environment,
clipboard data, editor recovery content, SSH secrets, credentials, protocol
payloads, or remote process content. Existing feature specs remain authoritative
for sensitive data.

## Migration Scope

M5.5 migrates the presentation of all existing release-critical surfaces:

- workspace opening, restoration, recent workspaces, and Quick Open;
- explorer, file operations, search, and source control;
- editor tabs, toolbars, find and replace, language results, conflicts, binary
  metadata, large-file preview, and recovery decisions;
- local terminal tabs, splits, selection, links, paste approval, process states,
  and load indicators;
- local persistent sessions, windows, panes, lifecycle actions, confirmations, and
  restoration;
- remote connection entry, recent connections, helper installation and repair,
  capability state, remote files, tasks, Git, language state, diagnostics, and
  direct-terminal fallback;
- remote persistent-session provider selection, discovery, hierarchy, panes,
  reconnect, stale state, and lifecycle actions;
- Problems, context, settings, empty states, loading, errors, and unavailable
  states; and
- application chrome, command center, theme selection, focus, and keyboard paths.

Migration may split the existing application view module into focused feature and
composition modules. It must not move domain behavior into `strukt-ui` or duplicate
existing state machines in presentation code.

## Performance Requirements

- Theme resolution and component style lookup are bounded and do not allocate per
  rendered cell, file row, or terminal cell.
- Theme switching completes without restarting background services or rebuilding
  feature state.
- Large explorer trees, diagnostics lists, search results, session histories, and
  terminal output retain existing virtualization, scheduling, and backpressure
  guarantees.
- Motion does not block input or background-event draining.
- Shell recomposition from panel resize or drawer promotion avoids unrelated
  feature-state cloning.
- M5.5 cannot regress the existing startup, terminal-load, editor-input, or remote
  reconnect release evidence without an explicit accepted release risk.

## Verification Strategy

### Theme and component verification

- Validate completeness and schema versioning for both built-in definitions.
- Validate protected-role and required-contrast relationships.
- Verify invalid definitions fall back atomically with a bounded diagnostic.
- Test hover, focus, pressed, selected, disabled, warning, destructive, stale, and
  unavailable component states.
- Enforce that feature presentation does not introduce raw application chrome
  colors outside documented editor and terminal parser boundaries.

### Shell-state verification

- Test activity and surface selection, sidebar collapse, context visibility,
  drawer open/close/resize, drawer promotion, split promotion, demotion, and focus
  return.
- Test invalid restored dimensions, unavailable contributions, removed features,
  stale surface identifiers, and incompatible presentation schema.
- Verify presentation transitions never start, restart, terminate, reconnect, or
  execute domain operations implicitly.

### Workflow regression verification

- Retain the existing complete M1 through M5 workspace test suite and deterministic
  smoke scripts.
- Add release-level local-workspace and remote-session smokes that exercise the new
  composition paths without weakening prior behavioral assertions.
- Require the final macOS, Ubuntu, and Windows hosted matrix at the exact PR head.

### Human verification

On supported macOS and Windows desktops, verify both built-in themes across:

- local file, editor, language, and terminal workflows;
- local persistent sessions;
- SSH workspace and failure recovery;
- remote native and tmux-backed persistent sessions;
- keyboard-only traversal and focus restoration;
- platform scaling and representative narrow and wide windows;
- reduced motion; and
- visual hierarchy, clipping, contrast, disabled states, destructive decisions,
  and stale/disconnected presentation.

Record limitations honestly where the native framework or automated environment
cannot prove platform behavior.

## Acceptance Criteria

M5.5 is complete only when:

1. The Quiet Precision shell matches the approved hierarchy in both durable
   mockups across local and remote workflows.
2. Dark and light built-in themes are complete, validated, selectable, and use the
   same versioned definition contract.
3. Theme switching is atomic and does not restart feature processes or services.
4. Reusable `strukt-ui` primitives replace feature-owned application chrome
   styling across every M1 through M5 surface.
5. The adaptive canvas, activity rail, contextual sidebar, on-demand context panel,
   status strip, and promoteable drawer obey the specified ownership rules.
6. Drawer tools promote to split or full canvas and return without losing
   feature-owned runtime state.
7. Capability removal does not leave empty navigation, invalid restored surfaces,
   or unrelated state loss.
8. Empty, loading, unavailable, stale, recoverable-error, destructive, and
   security-sensitive states follow the shared state language.
9. Keyboard paths, focus restoration, accessibility semantics, platform scaling,
   and reduced-motion behavior pass the documented human review.
10. Existing M1 through M5 automated suites and deterministic smokes pass without
    weakened assertions.
11. The exact final pull-request head passes hosted macOS, Ubuntu, and Windows CI.
12. Validation evidence documents visual review, accessibility review, performance
    checks, regression results, substantive agentic findings, and remaining Alpha
    limitations.

## Delivery Artifacts

The governed M5.5 delivery requires:

- this dedicated spec;
- a detailed implementation plan;
- a tracking issue;
- a feature branch and pull request linking the spec, plan, issue, mockups, and
  validation evidence;
- deterministic verification and final hosted matrix evidence;
- a substantive full-slice agentic review; and
- roadmap, tracker, README, and Public Alpha dependency updates.

## Dependency and Release Ordering

M5.5 depends on completed M1 through M5 behavior. The Public Alpha gate depends on
M5.5. M6 remains AI and workspace context after Alpha; the insertion of M5.5 does
not renumber the post-Alpha roadmap.
