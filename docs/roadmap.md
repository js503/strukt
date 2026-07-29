# Roadmap

## Purpose

This roadmap sequences `strukt` from its approved product foundation to its first
public alpha. It is the canonical view of milestone order, dependencies, status,
and related delivery artifacts.

The roadmap is outcome-based rather than date-based. A milestone advances when its
exit criteria are satisfied and verified, not when a target date arrives.

## Status Definitions

- **Complete:** the milestone exit criteria have been met and verification is
  recorded.
- **In progress:** implementation is underway on an approved plan.
- **Planned:** an approved implementation plan exists, but implementation has not
  started.
- **Shaping:** the milestone is being defined in a spec or architecture decision.
- **Not planned:** the outcome is sequenced, but its dedicated spec and
  implementation plan have not been written.
- **Blocked:** an unresolved dependency prevents meaningful progress.

## Milestone Sequence

| ID | Milestone | Status | Depends on | Primary outcome |
|---|---|---|---|---|
| M0 | Product and architecture foundation | Complete | — | Approved product model, spatial design, remote-development model, and delivery process |
| M1 | Native shell foundation | In progress | M0 | Cross-platform native shell proving capability boundaries, shell state, and semantic theming |
| M2 | Local development workspace | Not planned | M1 | Real local files, editing, PTY/ConPTY terminals, terminal rendering, and workspace persistence |
| M3 | Local persistent sessions | Not planned | M2 | Named local sessions with windows, split panes, detach/reattach, and restoration |
| M4 | SSH remote workspace | Not planned | M2 | A remote development box behaves as a first-class workspace over standard SSH |
| M5 | Remote persistent sessions | Not planned | M3, M4 | Multiple persistent sessions per remote host, reconnect recovery, and tmux interoperability |
| M6 | AI and workspace context | Not planned | M2, M4 | Optional, model-agnostic AI grounded in explicit local and remote workspace context |
| M7 | Plugin and MCP foundation | Not planned | M1, M6 | Sandboxed extensions, MCP discovery, permissions, and host-controlled contributions |
| M8 | Integrated developer workflows | Not planned | M2, M4, M7 | Git, tasks, logs, diagnostics, containers, and Kubernetes as cohesive workspace surfaces |
| M9 | Public alpha readiness | Not planned | M1–M8 | Installable, documented, supportable public alpha for macOS and Windows with Linux in the build pipeline |

## M0 — Product and Architecture Foundation

### Outcome

The product direction, Focus + Context spatial model, local-first boundaries,
remote-workspace model, persistent-session hierarchy, modularity requirements,
theming model, and repository delivery process are documented and approved.

### Exit Criteria

- The foundation spec is approved.
- The core workspace and remote-development interactions have reviewable mockups.
- The native UI framework decision has a documented validation path.
- The repository contains the forj-derived process, contribution, planning, and
  tracking structure.

### Related Artifacts

- Spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Architecture decision:
  [`decisions/0001-native-ui-framework.md`](decisions/0001-native-ui-framework.md)
- Mockups:
  [`mockups/workspace-shell/focus-context.html`](mockups/workspace-shell/focus-context.html),
  [`mockups/workspace-shell/remote-workspace.html`](mockups/workspace-shell/remote-workspace.html),
  and
  [`mockups/workspace-shell/remote-multiplexer.html`](mockups/workspace-shell/remote-multiplexer.html)
- Process:
  [`process/development-lifecycle.md`](process/development-lifecycle.md),
  [`process/review-standard.md`](process/review-standard.md), and
  [`process/merge-policy.md`](process/merge-policy.md)

## M1 — Native Shell Foundation

### Outcome

An executable Rust desktop shell validates the selected native GPU UI framework
across the supported architecture. It proves the Focus + Context layout,
capability registration, UI-independent shell state, semantic theme tokens, and
the boundaries that later feature modules will use.

Representative file, terminal, and AI surfaces are shell views only. Real
filesystem, editor, PTY, SSH, and model-provider behavior remain outside this
milestone.

### Exit Criteria

- The Cargo workspace and foundational crates build with the pinned toolchain.
- Capability registration, theme resolution, and shell-state behavior have unit
  tests.
- The native shell renders the approved spatial model and its core keyboard
  interactions.
- macOS, Windows, and Linux CI checks compile the relevant targets.
- UI framework validation results are recorded in ADR 0001.

### Related Artifacts

- Governing spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Implementation plan:
  [`plans/0001-native-shell-foundation.md`](plans/0001-native-shell-foundation.md)
- Architecture decision:
  [`decisions/0001-native-ui-framework.md`](decisions/0001-native-ui-framework.md)
- Spatial reference:
  [`mockups/workspace-shell/focus-context.html`](mockups/workspace-shell/focus-context.html)
- Issue: [#1 — M1: Native shell foundation](https://github.com/js503/strukt/issues/1)
- Pull request: [#2 — feat: add native shell foundation](https://github.com/js503/strukt/pull/2)

## M2 — Local Development Workspace

### Outcome

A developer can open a local workspace, navigate and edit real files, run local
shells, and restore workspace state without enabling AI or connecting to a cloud
service.

### Intended Scope

- Local workspace lifecycle and persisted layout
- Native file explorer, Quick Open, file watching, and foundational editing
- Unix PTY adapters and Windows ConPTY adapter
- GPU-rendered terminal surface with bounded scrollback
- Clear process, filesystem, and terminal capability contracts

### Exit Criteria

- Local file and terminal workflows operate on macOS and Windows.
- Linux remains supported by the shared contracts and CI pipeline.
- File, editor, and terminal state restore without corrupting the workspace.
- Terminal output cannot make file or editor interactions unresponsive.
- Platform adapter contract tests cover PTY/ConPTY and filesystem behavior.

### Related Artifacts

- Governing spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Dedicated spec: not yet created
- Implementation plan: not yet created
- Workspace reference:
  [`mockups/workspace-shell/focus-context.html`](mockups/workspace-shell/focus-context.html)

## M3 — Local Persistent Sessions

### Outcome

Local terminals become durable, named sessions rather than disposable panes.
A developer can manage multiple sessions, windows, and split panes and can
detach, reattach, and restore them through the workspace interface.

### Intended Scope

- Session → window → pane hierarchy
- Independent lifecycle, naming, layout, attention, and scrollback state
- Detach, reattach, rename, duplicate, restart, and terminate actions
- Application-restart restoration with explicit command restart policies
- Provider boundary reusable by native and tmux-backed implementations

### Exit Criteria

- Multiple named sessions remain isolated from one another.
- Window and split layouts restore after an application restart.
- Session lifecycle actions are reproducible through commands.
- Arbitrary commands never restart after a machine reboot without explicit policy.

### Related Artifacts

- Governing spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Dedicated spec: not yet created
- Implementation plan: not yet created
- Interaction reference:
  [`mockups/workspace-shell/remote-multiplexer.html`](mockups/workspace-shell/remote-multiplexer.html)

## M4 — SSH Remote Workspace

### Outcome

A developer can connect to a remote development box, including an AWS-backed EC2
instance, and use its files, terminals, Git state, tasks, and diagnostics through
the same workspace model used locally.

### Intended Scope

- Standard SSH configuration, keys, agent, known hosts, and `ProxyJump`
- Durable connection and recent-workspace management
- Visible local-versus-remote execution boundaries
- Terminal-only SSH fallback
- Optional, versioned per-user remote helper with no root requirement or public
  listening port
- Remote file access, search, Git, tasks, diagnostics, and capability negotiation

### Exit Criteria

- A Linux EC2 host can be opened using standard SSH configuration.
- Remote files and Quick Open operate on the remote filesystem.
- Helper installation or upgrade requires explicit consent.
- A helper failure retains usable terminal-only SSH access.
- Disconnects keep the local interface responsive and stale state visible.

### Related Artifacts

- Governing spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Dedicated spec: not yet created
- Implementation plan: not yet created
- Workspace reference:
  [`mockups/workspace-shell/remote-workspace.html`](mockups/workspace-shell/remote-workspace.html)

## M5 — Remote Persistent Sessions

### Outcome

One remote server can own multiple tmux-like persistent sessions. Sessions keep
running through SSH interruptions and local application restarts, and existing
tmux sessions can be discovered and attached through a common provider model.

### Intended Scope

- Remote helper ownership of native sessions and PTYs
- Reconnect, missed-output, sequence, and stale-state recovery
- Multiple sessions with windows and split panes on the same remote host
- Bounded buffering, backpressure, and per-pane scrollback
- Native and tmux-backed session providers with explicit capability differences
- Reboot restoration of definitions and history without implicit command restart

### Exit Criteria

- Sessions continue through SSH disconnect and local application restart.
- Concurrent remote sessions remain isolated under noisy output and reconnect.
- Layouts and available history restore after reconnect.
- Existing tmux sessions can be discovered and attached.
- Protocol upgrades and incompatible-helper fallback are verified.

### Related Artifacts

- Governing spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Dedicated spec: not yet created
- Implementation plan: not yet created
- Interaction reference:
  [`mockups/workspace-shell/remote-multiplexer.html`](mockups/workspace-shell/remote-multiplexer.html)

## M6 — AI and Workspace Context

### Outcome

AI becomes an optional workspace-native capability that understands explicitly
selected local or remote context and can use approved tools without becoming a
dependency of editing, terminals, or remote development.

### Intended Scope

- Model-agnostic providers, including local providers
- Workspace graph, open files, Git history, terminal history, diagnostics, and
  documentation context
- Visible context disclosure before provider transmission
- Shared permissions and approvals for tool execution
- Multiple conversations and agent workflows
- Provider and AI-module disablement without workspace degradation

### Exit Criteria

- At least one cloud provider and one local provider work behind the same contract.
- Context provenance and destination are visible before transmission.
- Tool actions use shared permissions and leave an auditable local record.
- Disabling AI preserves all non-AI workspace capabilities.

### Related Artifacts

- Governing spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Dedicated spec: not yet created
- Provider architecture decision: not yet created
- Implementation plan: not yet created

## M7 — Plugin and MCP Foundation

### Outcome

Third parties can extend `strukt` through sandboxed capabilities, MCP servers,
commands, workflows, providers, and host-controlled UI contributions without
compromising workspace integrity.

### Intended Scope

- MCP server discovery, lifecycle, tools, resources, prompts, and permissions
- Sandboxed plugin runtime and versioned host API
- Capability declarations and user/workspace grants
- Commands, menus, panes, views, inspectors, and theme-token contributions
- Failure isolation and safe enable/disable behavior

### Exit Criteria

- A reference extension contributes a command, a tool, and a host-controlled view.
- Permission grants are scoped, visible, revocable, and persisted safely.
- Plugin failure does not crash or corrupt the workspace.
- Disabling a plugin removes its contributions while preserving unrelated state.
- The plugin-runtime architecture decision records prototype evidence.

### Related Artifacts

- Governing spec:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Dedicated spec: not yet created
- Plugin runtime architecture decision: not yet created
- Implementation plan: not yet created

## M8 — Integrated Developer Workflows

### Outcome

Common engineering workflows become cohesive workspace surfaces rather than
collections of terminal commands and disconnected external tools.

### Intended Scope

- Git status, history, diff, staging, commit, and branch workflows
- Task runners, diagnostics, logs, and session-aware output
- Docker and Kubernetes explorers
- Documentation and prompt-library surfaces
- Commands and automation shared across built-in and extension-provided workflows

### Exit Criteria

- Git, task, and log workflows operate consistently in local and remote workspaces.
- Container and Kubernetes access uses explicit connection and permission scopes.
- Every primary action is keyboard-accessible and command-palette discoverable.
- Workflow state can be captured in workspace snapshots or reproducible commands.

### Related Artifacts

- Product foundation:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Dedicated spec: not yet created
- Implementation plan: not yet created

## M9 — Public Alpha Readiness

### Outcome

`strukt` is ready for external developers to install, evaluate, report problems,
and use for bounded local and remote development workflows.

### Intended Scope

- Signed or appropriately packaged macOS and Windows applications
- Linux build artifacts or documented build path from the shared pipeline
- First-run experience, connection onboarding, and failure recovery
- Accessibility, keyboard navigation, performance, and startup validation
- Security review of SSH, remote helper, permissions, plugins, and AI context
- Crash reporting and telemetry only when explicitly opt-in
- User, contributor, troubleshooting, and extension documentation
- Release, support, feedback, and compatibility policy

### Exit Criteria

- Packaged smoke tests pass on supported macOS and Windows versions.
- A human validates visual rendering and keyboard workflows on a supported Windows
  desktop.
- A new user can complete documented local and EC2-backed remote workflows.
- Critical accessibility and keyboard-only workflows are verified.
- Known limitations and experimental capabilities are documented.
- Security and privacy behavior matches the local-first product principles.
- Release artifacts, checksums, licenses, notices, and upgrade instructions are
  published.
- No unresolved release-blocking defects remain.

### Related Artifacts

- Product foundation:
  [`specs/0001-workspace-shell-and-remote-development.md`](specs/0001-workspace-shell-and-remote-development.md)
- Release criteria spec: not yet created
- Packaging and distribution plans: not yet created
- Implementation plan: not yet created

## Planning Rules

- A milestone cannot enter **In progress** until its dedicated behavior is covered
  by an approved spec and an executable implementation plan.
- Architecture decisions are written before a plan depends on an unresolved
  technology choice.
- Later milestones may be reshaped as earlier validation produces evidence, but
  dependency order and scope changes must be reflected here and in
  [`tracker.md`](tracker.md).
- Work may be split into smaller plans within one milestone when that keeps pull
  requests independently reviewable and verifiable.
- Public-alpha scope is defined by M9 exit criteria, not by implementing every item
  in the long-term product vision.

## Tracking

Execution status, issues, and pull requests are maintained in
[`tracker.md`](tracker.md). Specs define product behavior, plans define execution,
architecture decisions record consequential technical choices, and this roadmap
connects those artifacts into one delivery sequence.
