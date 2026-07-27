# Workspace Shell and Remote Development Foundation

- Status: Design approved; written-spec review pending
- Date: 2026-07-26
- Mockups: [`docs/mockups/workspace-shell/`](../mockups/workspace-shell/)

## Summary

`strukt` is an open-source, AI-native development interface in which terminals,
editors, agents, remote systems, logs, Git, documentation, and developer tools are
first-class workspace surfaces.

The product is a native desktop application for macOS, Windows, and Linux. It uses
a shared Rust core, a GPU-rendered interface, and platform-specific adapters rather
than Electron. Cloud services are optional; local and SSH-backed development must
work without a `strukt` cloud account.

This specification defines the foundational product experience, runtime boundaries,
remote workspace model, persistent terminal multiplexing model, extensibility
boundaries, and theming requirements. It intentionally does not require every
long-term feature to ship in the first implementation milestone.

## Goals

- Make the workspace interface, rather than the terminal, the center of development.
- Preserve a fast, keyboard-first experience across macOS, Windows, and Linux.
- Treat local and SSH-backed remote workspaces as equal product concepts.
- Make files, terminals, agents, and contextual tools immediately accessible.
- Support multiple persistent tmux-like terminal sessions on one remote server.
- Permit first-party features to be added, removed, or disabled without coupling the
  entire application to them.
- Provide a sandboxed extension model for tools, workflows, MCP servers, and UI
  contributions.
- Make theming a shared system capability used by built-in and third-party surfaces.
- Keep workspace state and control local by default.

## Non-goals

- Shipping every proposed built-in feature in the first vertical slice.
- Requiring a hosted control plane for local or remote development.
- Reimplementing every tmux command or configuration behavior exactly.
- Automatically restarting arbitrary commands after a remote-machine reboot.
- Defining the complete public plugin SDK in the first milestone.
- Defining collaboration, cloud synchronization, or marketplace operations.
- Finalizing pixel-perfect visual styling before the native UI framework is selected.

## Product Experience

### Adaptive workspace

Opening an existing workspace restores its last focused view and layout. Opening a
new workspace shows a lightweight overview from which the user can open files,
start a terminal, connect an agent, or choose another tool.

The interface follows the approved **Focus + Context** spatial model:

- A stable left activity rail exposes files, search, Git, sessions, tasks,
  connections, plugins, and settings.
- A resizable file explorer is pinned by default and can collapse without losing
  workspace state.
- The center is the dominant canvas for editors, terminals, logs, documentation,
  dashboards, and plugin views.
- A contextual right panel hosts AI conversation, workspace context, diagnostics,
  inspectors, and approvals.
- Secondary tools can open in a bottom drawer or a temporary split.
- A universal command palette exposes every action and view.
- Quick Open provides keyboard-first access to workspace files.

The approved spatial reference is
[`focus-context.html`](../mockups/workspace-shell/focus-context.html).

### Files

File access is foundational rather than an optional feature.

- The explorer is the first activity-rail destination.
- It identifies whether the displayed filesystem is local or remote.
- It supports resize, collapse, restore, keyboard navigation, and Quick Open.
- Remote paths are operated on the remote host; the client does not silently mirror
  an entire workspace locally.
- File actions and editor state must remain responsive while background terminals or
  agents produce output.

The approved active-workspace reference is
[`remote-workspace.html`](../mockups/workspace-shell/remote-workspace.html).

### Connections

Connections have a durable view that lists known SSH hosts, connection health, and
recent remote workspaces. The same actions are available through the command
palette so expert users are never forced through a dashboard.

The connection view discovers standard SSH configuration and supports:

- SSH keys and the local SSH agent
- known-host verification
- aliases from SSH configuration
- jump hosts and `ProxyJump`
- recent remote workspace paths
- explicit connect, reconnect, and disconnect actions

### Local and remote boundaries

Local and remote workspaces share the same interaction model, but the current
execution boundary must always be visible. A remote workspace identifies its host
in the window header, explorer root, terminal chrome, AI context, and status bar.

Actions that cross the boundary, install a helper, forward a port, expose context to
an AI provider, or transfer a file require an explicit and understandable scope.

## Desktop Platforms

`strukt` must produce installable desktop applications for:

- macOS
- Windows
- Linux

macOS and Windows builds are required for the first public alpha. Linux remains a
supported architecture and distribution target from the beginning.

A shared Rust core owns product behavior. Platform adapters isolate:

- Unix PTYs on macOS and Linux
- ConPTY on Windows
- native credential storage
- filesystem watching
- windowing, menus, shortcuts, notifications, and packaging
- SSH-agent and platform-integration differences

Platform-specific adapters must implement shared contracts so platform behavior can
be verified independently.

## Runtime Architecture

### Minimal kernel

The kernel owns only capabilities that every workspace and feature requires:

- application lifecycle
- GPU renderer and native UI primitives
- workspace identity and lifecycle
- capability registry
- typed command and event bus
- persistence and schema migration
- permissions and consent
- plugin runtime and isolation

### Platform adapters

Platform adapters provide operating-system capabilities without leaking
platform-specific behavior into feature modules:

- PTY or ConPTY
- filesystem and file watching
- credentials and keychains
- SSH integration
- process lifecycle
- windowing and notifications

### First-party feature modules

Major product capabilities are separately owned modules, including:

- file explorer and editor
- terminal and session manager
- remote workspaces
- Git
- AI and agents
- logs and tasks
- containers and Kubernetes
- documentation

Modules communicate through typed capabilities, commands, and events rather than
importing one another's internal state. A module declares the capabilities it
provides and requires, its permissions, its commands, its UI contributions, and its
persisted-state schema.

Except for kernel capabilities, first-party modules can be enabled or disabled at
the user or workspace level. Removing a module removes its commands and views
without corrupting unrelated workspace state.

## Remote Workspace Architecture

### SSH transport

The local desktop application connects through standard SSH. The local application
renders the interface while operations that own remote state execute on the remote
machine.

The connection starts in terminal-only mode. With explicit consent, `strukt`
installs or updates a small versioned helper under the remote user's home directory.
The helper:

- requires no root access
- exposes no public listening port
- communicates through the protected SSH channel or a user-owned local socket
- performs remote filesystem operations, Git, search, tasks, diagnostics, port
  forwarding, and PTY management
- reports its protocol version and capabilities

If the helper cannot be installed or its protocol cannot be negotiated, plain SSH
terminal access remains available.

### Remote data flow

1. The client authenticates using standard SSH configuration and verifies the host.
2. The client discovers or bootstraps the versioned remote helper.
3. The helper advertises capabilities and the current workspace/session inventory.
4. The client subscribes only to the files, diagnostics, processes, and terminal
   output required by visible or active views.
5. State updates carry stable identifiers and sequence numbers.
6. After reconnecting, the client requests changes and terminal output after its
   last acknowledged sequence number.
7. The local interface marks stale state clearly until synchronization completes.

Large or high-volume streams must apply backpressure and bounded buffering so a
noisy terminal cannot make the interface unresponsive.

## Persistent Remote Terminal Multiplexing

One remote server is a workspace target. It can own multiple named terminal
sessions with tmux-like semantics:

```text
Remote server
└── Named persistent session
    └── Window
        └── Split pane / PTY
```

Each session has:

- a stable identifier and editable name
- independent lifecycle and status
- one or more windows
- one or more PTY panes per window
- layout state
- working-directory and environment metadata
- bounded scrollback
- unread output and attention state
- detach, reattach, rename, duplicate, restart, and terminate actions

The remote helper owns native sessions and their PTYs. Sessions continue running
after an SSH interruption or after the local desktop application closes. Reopening
the workspace restores the session inventory, layouts, and output that occurred
while disconnected.

After a remote-machine reboot, `strukt` restores session definitions, layouts, and
available history. It does not restart arbitrary commands unless the user has
explicitly marked a command or task as restartable.

Existing tmux installations are supported through a provider interface. Native and
tmux-backed sessions use the same user-facing hierarchy where their capabilities
overlap. Provider-specific limitations are shown rather than hidden.

The approved interaction reference is
[`remote-multiplexer.html`](../mockups/workspace-shell/remote-multiplexer.html).

## AI and Context

AI is a first-party feature module rather than a kernel dependency.

- Providers are model-agnostic and replaceable.
- Disabling AI must not disable terminals, editing, remote development, or other
  workspace capabilities.
- Context can include the workspace graph, Git history, open files, terminal
  history, diagnostics, documentation, and explicitly enabled memory.
- The interface shows which local or remote context will be sent to a provider.
- Tool execution and destructive operations use the shared permission system.
- Local providers can operate without cloud services.

## Extensibility

External extensions are sandboxed and capability-based.

- MCP servers contribute tools, resources, prompts, and context integrations.
- Sandboxed logic plugins contribute commands, workflows, providers, and services.
- UI extensions contribute views, panes, commands, menus, and inspectors through
  host-controlled native UI contracts.
- Extensions declare permissions before accessing files, processes, networks,
  credentials, remote hosts, AI context, or UI contribution points.
- The host can disable an extension globally or for one workspace.

The initial plugin runtime should favor a sandboxed portable format such as the
WebAssembly component model. The final runtime choice belongs in a dedicated
architecture decision record after prototyping validates startup time, isolation,
host APIs, and cross-platform support.

Built-in modules may use richer internal Rust contracts, but must follow the same
capability boundaries that prevent unrelated features from depending on their
internals.

## Theming

Theming is a platform capability shared by the kernel, feature modules, and
extensions. Components must consume semantic tokens instead of hard-coded visual
values.

Theme coverage includes:

- application chrome
- panels, controls, focus, and selection states
- typography and interface density
- editor syntax
- terminal ANSI colors
- Git and diagnostic states
- agent, connection, and session states
- icons

Themes can provide light and dark variants and follow the operating-system setting.
Theme packages are installable without granting code-execution permissions.
Third-party UI contributions receive host-provided theme tokens so they remain
legible and consistent.

## Persistence

Local workspace metadata is stored locally by default. Persisted state includes:

- workspace identity and target
- open views and layout
- enabled modules and extensions
- editor state
- connection metadata without private secrets
- session identifiers and presentation state
- theme and user preferences

Secrets remain in native credential storage or existing SSH facilities. Private keys
are not copied into workspace state. Remote helpers store only the state required to
own remote sessions and resume the remote workspace.

Persisted schemas are versioned. A missing or disabled feature does not make the
workspace unreadable; its opaque state can be preserved until the feature returns or
the user removes it.

## Failure Handling

- **SSH interruption:** keep the local UI responsive, mark remote state stale, and
  reconnect with bounded backoff.
- **Remote helper unavailable:** retain terminal-only SSH and offer an explicit
  repair or upgrade action.
- **Protocol mismatch:** negotiate a compatible version, update with consent, or
  fall back without corrupting state.
- **Remote process exit:** preserve exit status and scrollback until the user closes
  or restarts the pane.
- **Noisy output:** apply backpressure and bounded scrollback independently per
  pane.
- **Disabled module:** remove its contributions while preserving unrelated state.
- **Invalid theme:** fall back to a built-in accessible theme and identify the
  invalid package.
- **Plugin failure:** isolate the failure, disable the contribution, and keep the
  workspace operational.

## Security Requirements

- Verify SSH host identity using standard known-host behavior.
- Never expose the remote helper on a public network interface.
- Require explicit consent before remote helper installation or upgrade.
- Run the helper with the connected user's privileges and no elevation.
- Scope plugin and AI access through declared capabilities and permissions.
- Keep private keys and credentials outside workspace persistence.
- Make local-to-remote and AI-provider data movement visible.
- Log security-relevant grants and remote helper lifecycle actions locally.

## Verification Strategy

The implementation plan must include:

- unit tests for capability registration, module lifecycle, persistence migration,
  theme resolution, and permission evaluation
- contract tests for platform adapters
- PTY tests on Unix and ConPTY tests on Windows
- SSH integration tests against disposable remote hosts
- disconnect, reconnect, missed-output, and stale-state recovery tests
- concurrent remote session isolation tests
- session/window/pane lifecycle and layout restoration tests
- remote-helper upgrade and protocol-compatibility tests
- tmux-provider discovery and attachment tests
- plugin isolation and disabled-module tests
- keyboard navigation and accessibility checks
- visual regression coverage for built-in light and dark themes
- packaged smoke tests on macOS, Windows, and Linux

## Acceptance Criteria

The foundation is acceptable when:

1. The native desktop shell runs as packaged software on macOS and Windows, with
   Linux supported by the shared architecture and build pipeline.
2. A user can open a local workspace and access files, the command palette, and a
   terminal without enabling AI.
3. A user can connect to a Linux EC2 host using standard SSH configuration.
4. A remote workspace clearly identifies its execution boundary throughout the UI.
5. The remote file explorer and Quick Open operate on the remote filesystem.
6. The remote helper can be installed without root access and exposes no public
   port.
7. One remote server can run multiple named persistent sessions, each with windows
   and split PTY panes.
8. Sessions continue through SSH disconnects and local application restarts.
9. Existing tmux sessions can be discovered and attached through the provider
   interface.
10. AI and at least one other non-kernel feature can be disabled without breaking
    the workspace.
11. Built-in light and dark themes cover the shell, editor, terminal, diagnostics,
    and contributed UI tokens.
12. A failed plugin, invalid theme, or unavailable remote helper does not crash or
    corrupt the workspace.

## Implementation Planning Boundary

This specification defines the product and architectural direction. The
implementation plan must decompose it into independently verifiable vertical
milestones rather than attempting to ship the complete vision in one change.

The first milestone should prove the native shell, capability boundaries, local
workspace, file access, local PTY, and theme tokens. Remote helper and persistent
session work should follow behind stable local contracts, while macOS and Windows
verification begins with the first executable shell.
