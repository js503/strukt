# Native Shell Foundation Implementation Plan

- Status: Complete

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first installable-shaped `strukt` executable: a GPU-rendered
Focus + Context shell with framework-independent capabilities, shell state, and
semantic light/dark themes.

**Architecture:** A Rust workspace separates domain state from the Iced application.
`strukt-core` owns the capability registry, `strukt-theme` owns semantic visual
tokens, `strukt-shell` owns UI-independent shell state, and `strukt-app` is the only
crate coupled to Iced. This milestone renders representative file, terminal, and AI
surfaces but does not yet access the filesystem or spawn a PTY.

**Tech Stack:** Rust 1.97.1, Rust 2024 edition, Cargo resolver 3, Iced 0.14 with
`wgpu`, `tokio`, and `advanced` features, GitHub Actions, MIT license.

---

## Scope boundary

This is the first independently testable sub-project from
`docs/specs/0001-workspace-shell-and-remote-development.md`.

Included:

- reproducible Rust workspace and toolchain
- capability registration and enable/disable state
- semantic light and dark theme tokens
- framework-independent shell state transitions
- native GPU-rendered Focus + Context application shell
- keyboard actions for primary shell panels
- macOS, Windows, and Linux compile/test CI
- UI-framework validation record

Excluded:

- real filesystem enumeration and editing
- PTY/ConPTY creation and terminal emulation
- SSH, remote helper, and persistent remote multiplexing
- Git, AI-provider, container, Kubernetes, and MCP implementations
- packaging, signing, notarization, and auto-update
- third-party plugin loading

Those excluded capabilities depend on the boundaries proven here and receive
separate implementation plans.

## File map

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
rustfmt.toml
LICENSE
crates/
├── strukt-core/
│   ├── Cargo.toml
│   ├── src/lib.rs
│   ├── src/capability.rs
│   └── tests/capability_registry.rs
├── strukt-theme/
│   ├── Cargo.toml
│   ├── src/lib.rs
│   ├── src/tokens.rs
│   └── tests/builtin_themes.rs
├── strukt-shell/
│   ├── Cargo.toml
│   ├── src/lib.rs
│   ├── src/state.rs
│   └── tests/shell_state.rs
└── strukt-app/
    ├── Cargo.toml
    └── src/
        ├── main.rs
        ├── app.rs
        └── view.rs
.github/workflows/ci.yml
docs/decisions/0001-native-ui-framework.md
README.md
docs/tracker.md
```

Responsibilities:

- `strukt-core`: feature identity and enablement; no UI dependencies.
- `strukt-theme`: semantic color values; no UI dependencies.
- `strukt-shell`: panel/activity state transitions; no UI dependencies.
- `strukt-app`: Iced event loop, widgets, shortcuts, and token conversion.

### Task 1: Bootstrap the reproducible Rust workspace

**Files:**

- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Create: `LICENSE`
- Create: `Cargo.toml`
- Create: `crates/strukt-core/Cargo.toml`
- Create: `crates/strukt-core/src/lib.rs`
- Create: `crates/strukt-theme/Cargo.toml`
- Create: `crates/strukt-theme/src/lib.rs`
- Create: `crates/strukt-shell/Cargo.toml`
- Create: `crates/strukt-shell/src/lib.rs`
- Create: `crates/strukt-app/Cargo.toml`
- Create: `crates/strukt-app/src/main.rs`

- [x] **Step 1: Install the pinned Rust toolchain**

The current development machine does not have `rustc` or `cargo`. Request approval
before installing outside the repository, then run:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
  sh -s -- -y --profile minimal --default-toolchain 1.97.1
```

Start a new shell or load Cargo's environment, then verify:

```bash
rustc --version
cargo --version
```

Expected: `rustc 1.97.1` and its matching Cargo release.

- [x] **Step 2: Pin the toolchain and formatter**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.97.1"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

Create `rustfmt.toml`:

```toml
edition = "2024"
max_width = 100
use_field_init_shorthand = true
use_try_shorthand = true
```

- [x] **Step 3: Add the MIT license**

Create `LICENSE`:

```text
MIT License

Copyright (c) 2026 strukt contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [x] **Step 4: Define the Cargo workspace**

Create root `Cargo.toml`:

```toml
[workspace]
members = [
  "crates/strukt-app",
  "crates/strukt-core",
  "crates/strukt-shell",
  "crates/strukt-theme",
]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.97"
license = "MIT"
repository = "https://github.com/js503/strukt"

[workspace.dependencies]
iced = { version = "0.14.0", features = ["advanced", "tokio", "wgpu"] }
strukt-core = { path = "crates/strukt-core" }
strukt-shell = { path = "crates/strukt-shell" }
strukt-theme = { path = "crates/strukt-theme" }
thiserror = "2.0"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = "deny"
pedantic = "deny"
```

Create the three library manifests with this pattern, changing `name` for each
crate:

```toml
[package]
name = "strukt-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lints]
workspace = true
```

`crates/strukt-shell/Cargo.toml` additionally contains:

```toml
[dependencies]
strukt-core.workspace = true
strukt-theme.workspace = true
```

Create `crates/strukt-app/Cargo.toml`:

```toml
[package]
name = "strukt-app"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
iced.workspace = true
strukt-core.workspace = true
strukt-shell.workspace = true
strukt-theme.workspace = true

[lints]
workspace = true
```

- [x] **Step 5: Add compileable crate entry points**

Each library `src/lib.rs` initially contains:

```rust
#![forbid(unsafe_code)]
```

Create `crates/strukt-app/src/main.rs`:

```rust
#![forbid(unsafe_code)]

fn main() {
    println!("strukt native shell bootstrap");
}
```

- [x] **Step 6: Verify workspace metadata and the bootstrap build**

Run:

```bash
cargo metadata --no-deps --format-version 1
cargo fmt --all --check
cargo check --workspace
```

Expected: four workspace packages, no formatting differences, and a successful
development build.

- [x] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml LICENSE crates
git commit -m "build: bootstrap native Rust workspace"
```

### Task 2: Implement the capability registry with TDD

**Files:**

- Create: `crates/strukt-core/src/capability.rs`
- Modify: `crates/strukt-core/src/lib.rs`
- Create: `crates/strukt-core/tests/capability_registry.rs`

- [x] **Step 1: Write the failing capability tests**

Create `crates/strukt-core/tests/capability_registry.rs`:

```rust
use strukt_core::{
    CapabilityDescriptor, CapabilityId, CapabilityRegistry, RegistryError,
};

#[test]
fn registered_capabilities_use_their_default_state() {
    let mut registry = CapabilityRegistry::new();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::FILES, true))
        .unwrap();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::AI, false))
        .unwrap();

    assert!(registry.is_enabled(CapabilityId::FILES));
    assert!(!registry.is_enabled(CapabilityId::AI));
}

#[test]
fn explicit_enablement_overrides_the_default() {
    let mut registry = CapabilityRegistry::new();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::AI, false))
        .unwrap();

    registry.set_enabled(CapabilityId::AI, true).unwrap();

    assert!(registry.is_enabled(CapabilityId::AI));
}

#[test]
fn duplicate_registration_is_rejected() {
    let mut registry = CapabilityRegistry::new();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::FILES, true))
        .unwrap();

    let error = registry
        .register(CapabilityDescriptor::new(CapabilityId::FILES, true))
        .unwrap_err();

    assert_eq!(error, RegistryError::Duplicate(CapabilityId::FILES));
}
```

- [x] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p strukt-core --test capability_registry
```

Expected: compilation fails because the capability types are not exported.

- [x] **Step 3: Implement the minimal capability model**

Create `crates/strukt-core/src/capability.rs`:

```rust
use std::collections::BTreeMap;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CapabilityId(&'static str);

impl CapabilityId {
    pub const AI: Self = Self("ai");
    pub const CONNECTIONS: Self = Self("connections");
    pub const FILES: Self = Self("files");
    pub const TERMINAL: Self = Self("terminal");
    pub const THEMES: Self = Self("themes");

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub enabled_by_default: bool,
}

impl CapabilityDescriptor {
    pub const fn new(id: CapabilityId, enabled_by_default: bool) -> Self {
        Self {
            id,
            enabled_by_default,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CapabilityState {
    descriptor: CapabilityDescriptor,
    override_enabled: Option<bool>,
}

#[derive(Debug, Default)]
pub struct CapabilityRegistry {
    capabilities: BTreeMap<CapabilityId, CapabilityState>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, descriptor: CapabilityDescriptor) -> Result<(), RegistryError> {
        if self.capabilities.contains_key(&descriptor.id) {
            return Err(RegistryError::Duplicate(descriptor.id));
        }

        self.capabilities.insert(
            descriptor.id,
            CapabilityState {
                descriptor,
                override_enabled: None,
            },
        );
        Ok(())
    }

    pub fn set_enabled(
        &mut self,
        id: CapabilityId,
        enabled: bool,
    ) -> Result<(), RegistryError> {
        let state = self
            .capabilities
            .get_mut(&id)
            .ok_or(RegistryError::Unknown(id))?;
        state.override_enabled = Some(enabled);
        Ok(())
    }

    pub fn is_enabled(&self, id: CapabilityId) -> bool {
        self.capabilities.get(&id).is_some_and(|state| {
            state
                .override_enabled
                .unwrap_or(state.descriptor.enabled_by_default)
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum RegistryError {
    #[error("capability already registered: {0:?}")]
    Duplicate(CapabilityId),
    #[error("unknown capability: {0:?}")]
    Unknown(CapabilityId),
}
```

Modify `crates/strukt-core/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

mod capability;

pub use capability::{
    CapabilityDescriptor, CapabilityId, CapabilityRegistry, RegistryError,
};
```

Add `thiserror.workspace = true` to `crates/strukt-core/Cargo.toml`.

- [x] **Step 4: Run the focused and crate tests**

Run:

```bash
cargo test -p strukt-core --test capability_registry
cargo test -p strukt-core
```

Expected: three focused tests pass and the crate test suite passes.

- [x] **Step 5: Run static verification**

```bash
cargo fmt --all --check
cargo clippy -p strukt-core --all-targets -- -D warnings
```

Expected: both commands exit successfully with no diagnostics.

- [x] **Step 6: Commit**

```bash
git add crates/strukt-core
git commit -m "feat: add capability registry"
```

### Task 3: Implement semantic theme tokens with TDD

**Files:**

- Create: `crates/strukt-theme/src/tokens.rs`
- Modify: `crates/strukt-theme/src/lib.rs`
- Create: `crates/strukt-theme/tests/builtin_themes.rs`

- [x] **Step 1: Write the failing theme tests**

Create `crates/strukt-theme/tests/builtin_themes.rs`:

```rust
use strukt_theme::{ThemeMode, ThemeTokens};

#[test]
fn light_and_dark_themes_have_distinct_surfaces() {
    let light = ThemeTokens::builtin(ThemeMode::Light);
    let dark = ThemeTokens::builtin(ThemeMode::Dark);

    assert_ne!(light.canvas, dark.canvas);
    assert_ne!(light.text_primary, dark.text_primary);
}

#[test]
fn terminal_and_connection_tokens_are_semantic() {
    let theme = ThemeTokens::builtin(ThemeMode::Dark);

    assert_ne!(theme.terminal_background, theme.panel);
    assert_ne!(theme.connection_remote, theme.status_warning);
}
```

- [x] **Step 2: Run the test to verify it fails**

```bash
cargo test -p strukt-theme --test builtin_themes
```

Expected: compilation fails because `ThemeMode` and `ThemeTokens` do not exist.

- [x] **Step 3: Implement immutable semantic tokens**

Create `crates/strukt-theme/src/tokens.rs`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeTokens {
    pub canvas: Rgb,
    pub panel: Rgb,
    pub panel_active: Rgb,
    pub border: Rgb,
    pub text_primary: Rgb,
    pub text_muted: Rgb,
    pub accent: Rgb,
    pub focus: Rgb,
    pub terminal_background: Rgb,
    pub connection_remote: Rgb,
    pub status_success: Rgb,
    pub status_warning: Rgb,
}

impl ThemeTokens {
    pub const fn builtin(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self {
                canvas: Rgb::new(246, 247, 249),
                panel: Rgb::new(255, 255, 255),
                panel_active: Rgb::new(235, 240, 247),
                border: Rgb::new(207, 213, 223),
                text_primary: Rgb::new(27, 35, 48),
                text_muted: Rgb::new(95, 104, 119),
                accent: Rgb::new(31, 111, 235),
                focus: Rgb::new(31, 111, 235),
                terminal_background: Rgb::new(13, 17, 23),
                connection_remote: Rgb::new(35, 134, 54),
                status_success: Rgb::new(35, 134, 54),
                status_warning: Rgb::new(154, 103, 0),
            },
            ThemeMode::Dark => Self {
                canvas: Rgb::new(13, 17, 23),
                panel: Rgb::new(22, 27, 34),
                panel_active: Rgb::new(33, 38, 45),
                border: Rgb::new(48, 54, 61),
                text_primary: Rgb::new(240, 246, 252),
                text_muted: Rgb::new(139, 148, 158),
                accent: Rgb::new(88, 166, 255),
                focus: Rgb::new(88, 166, 255),
                terminal_background: Rgb::new(9, 12, 16),
                connection_remote: Rgb::new(126, 231, 135),
                status_success: Rgb::new(126, 231, 135),
                status_warning: Rgb::new(227, 179, 65),
            },
        }
    }
}
```

Modify `crates/strukt-theme/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

mod tokens;

pub use tokens::{Rgb, ThemeMode, ThemeTokens};
```

- [x] **Step 4: Run tests and static verification**

```bash
cargo test -p strukt-theme
cargo fmt --all --check
cargo clippy -p strukt-theme --all-targets -- -D warnings
```

Expected: two theme tests pass and both static checks exit successfully.

- [x] **Step 5: Commit**

```bash
git add crates/strukt-theme
git commit -m "feat: add semantic theme tokens"
```

### Task 4: Implement framework-independent shell state with TDD

**Files:**

- Create: `crates/strukt-shell/src/state.rs`
- Modify: `crates/strukt-shell/src/lib.rs`
- Create: `crates/strukt-shell/tests/shell_state.rs`

- [x] **Step 1: Write the failing shell-state tests**

Create `crates/strukt-shell/tests/shell_state.rs`:

```rust
use strukt_shell::{Activity, ShellAction, ShellState};
use strukt_theme::ThemeMode;

#[test]
fn selecting_files_keeps_the_explorer_visible() {
    let mut state = ShellState::default();
    state.apply(ShellAction::SelectActivity(Activity::Files));

    assert_eq!(state.active_activity, Activity::Files);
    assert!(state.explorer_visible);
}

#[test]
fn panels_toggle_independently() {
    let mut state = ShellState::default();

    state.apply(ShellAction::ToggleContext);
    state.apply(ShellAction::ToggleDrawer);

    assert!(!state.context_visible);
    assert!(state.drawer_visible);
}

#[test]
fn theme_toggle_switches_between_builtin_modes() {
    let mut state = ShellState::default();
    assert_eq!(state.theme_mode, ThemeMode::Dark);

    state.apply(ShellAction::ToggleTheme);

    assert_eq!(state.theme_mode, ThemeMode::Light);
}
```

- [x] **Step 2: Run the test to verify it fails**

```bash
cargo test -p strukt-shell --test shell_state
```

Expected: compilation fails because the shell-state types do not exist.

- [x] **Step 3: Implement the shell reducer**

Create `crates/strukt-shell/src/state.rs`:

```rust
use strukt_theme::ThemeMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activity {
    Files,
    Search,
    SourceControl,
    Sessions,
    Tasks,
    Connections,
    Extensions,
    Settings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellAction {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellState {
    pub active_activity: Activity,
    pub explorer_visible: bool,
    pub context_visible: bool,
    pub drawer_visible: bool,
    pub theme_mode: ThemeMode,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            active_activity: Activity::Files,
            explorer_visible: true,
            context_visible: true,
            drawer_visible: false,
            theme_mode: ThemeMode::Dark,
        }
    }
}

impl ShellState {
    pub fn apply(&mut self, action: ShellAction) {
        match action {
            ShellAction::SelectActivity(activity) => {
                self.active_activity = activity;
                if activity == Activity::Files {
                    self.explorer_visible = true;
                }
            }
            ShellAction::ToggleContext => self.context_visible = !self.context_visible,
            ShellAction::ToggleDrawer => self.drawer_visible = !self.drawer_visible,
            ShellAction::ToggleExplorer => self.explorer_visible = !self.explorer_visible,
            ShellAction::ToggleTheme => {
                self.theme_mode = match self.theme_mode {
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::Dark => ThemeMode::Light,
                };
            }
        }
    }
}
```

Modify `crates/strukt-shell/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

mod state;

pub use state::{Activity, ShellAction, ShellState};
```

- [x] **Step 4: Run tests and static verification**

```bash
cargo test -p strukt-shell
cargo fmt --all --check
cargo clippy -p strukt-shell --all-targets -- -D warnings
```

Expected: three shell-state tests pass and both static checks exit successfully.

- [x] **Step 5: Commit**

```bash
git add crates/strukt-shell
git commit -m "feat: add workspace shell state"
```

### Task 5: Render the native Focus + Context application

**Files:**

- Modify: `crates/strukt-app/src/main.rs`
- Create: `crates/strukt-app/src/app.rs`
- Create: `crates/strukt-app/src/view.rs`

- [x] **Step 1: Define application state and messages**

Create `crates/strukt-app/src/app.rs`:

```rust
use iced::keyboard::{self, Key};
use iced::{Subscription, Theme};
use strukt_core::{CapabilityDescriptor, CapabilityId, CapabilityRegistry};
use strukt_shell::{Activity, ShellAction, ShellState};
use strukt_theme::ThemeMode;

#[derive(Debug)]
pub struct StruktApp {
    pub capabilities: CapabilityRegistry,
    pub shell: ShellState,
}

#[derive(Clone, Debug)]
pub enum Message {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
    Keyboard(keyboard::Event),
}

impl Default for StruktApp {
    fn default() -> Self {
        let mut capabilities = CapabilityRegistry::new();
        for descriptor in [
            CapabilityDescriptor::new(CapabilityId::FILES, true),
            CapabilityDescriptor::new(CapabilityId::TERMINAL, true),
            CapabilityDescriptor::new(CapabilityId::THEMES, true),
            CapabilityDescriptor::new(CapabilityId::CONNECTIONS, true),
            CapabilityDescriptor::new(CapabilityId::AI, true),
        ] {
            capabilities
                .register(descriptor)
                .expect("built-in capability identifiers must be unique");
        }

        Self {
            capabilities,
            shell: ShellState::default(),
        }
    }
}

impl StruktApp {
    pub fn update(&mut self, message: Message) {
        let action = match message {
            Message::SelectActivity(activity) => Some(ShellAction::SelectActivity(activity)),
            Message::ToggleContext => Some(ShellAction::ToggleContext),
            Message::ToggleDrawer => Some(ShellAction::ToggleDrawer),
            Message::ToggleExplorer => Some(ShellAction::ToggleExplorer),
            Message::ToggleTheme => Some(ShellAction::ToggleTheme),
            Message::Keyboard(keyboard::Event::KeyPressed {
                key, modifiers, ..
            }) if modifiers.command() => match key.as_ref() {
                Key::Character("b") => Some(ShellAction::ToggleExplorer),
                Key::Character("j") => Some(ShellAction::ToggleDrawer),
                Key::Character("\\") => Some(ShellAction::ToggleContext),
                _ => None,
            },
            Message::Keyboard(_) => None,
        };
        if let Some(action) = action {
            self.shell.apply(action);
        }
    }

    pub fn theme(&self) -> Theme {
        match self.shell.theme_mode {
            ThemeMode::Light => Theme::Light,
            ThemeMode::Dark => Theme::Dark,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().map(Message::Keyboard)
    }
}
```

- [x] **Step 2: Render the approved shell hierarchy**

Create `crates/strukt-app/src/view.rs`:

```rust
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Fill, Length};
use strukt_shell::Activity;
use strukt_theme::{Rgb, ThemeTokens};

use crate::app::{Message, StruktApp};

fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

fn panel_style(
    tokens: ThemeTokens,
    background: Rgb,
) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color(background))),
        text_color: Some(color(tokens.text_primary)),
        border: Border {
            color: color(tokens.border),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn activity_button(label: &'static str, activity: Activity) -> Element<'static, Message> {
    button(text(label))
        .width(Fill)
        .on_press(Message::SelectActivity(activity))
        .into()
}

pub fn view(app: &StruktApp) -> Element<'_, Message> {
    let tokens = ThemeTokens::builtin(app.shell.theme_mode);
    let header = container(
        row![
            text("strukt").size(16),
            text("  /  local workspace").size(13),
            Space::new().width(Fill),
            button("Toggle theme").on_press(Message::ToggleTheme),
            button("Context").on_press(Message::ToggleContext),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Fill)
    .style(panel_style(tokens, tokens.panel));

    let activity_rail = container(
        column![
            activity_button("Files", Activity::Files),
            activity_button("Search", Activity::Search),
            activity_button("Git", Activity::SourceControl),
            activity_button("Sessions", Activity::Sessions),
            activity_button("Tasks", Activity::Tasks),
            activity_button("Connect", Activity::Connections),
            activity_button("Extend", Activity::Extensions),
            Space::new().height(Fill),
            activity_button("Settings", Activity::Settings),
        ]
        .spacing(6),
    )
    .padding(6)
    .width(Length::Fixed(92.0))
    .style(panel_style(tokens, tokens.panel));

    let explorer: Element<'_, Message> = if app.shell.explorer_visible {
        container(
            column![
                row![
                    text("EXPLORER"),
                    Space::new().width(Fill),
                    button("×").on_press(Message::ToggleExplorer),
                ],
                text("STRUKT"),
                scrollable(column![
                    text("▾ crates"),
                    text("  ▸ strukt-app"),
                    text("  ▸ strukt-core"),
                    text("  ▸ strukt-shell"),
                    text("  ▸ strukt-theme"),
                    text("▸ docs"),
                    text("  README.md"),
                    text("  Cargo.toml"),
                ]
                .spacing(6)),
            ]
            .spacing(10),
        )
        .padding(10)
        .width(Length::Fixed(235.0))
        .style(panel_style(tokens, tokens.panel))
        .into()
    } else {
        container(Space::new()).width(Length::Shrink).into()
    };

    let primary = container(
        column![
            text("Workspace shell").size(22),
            text("The primary canvas adapts to files, terminals, logs, and tools."),
            Space::new().height(Fill),
            text("Open a file or choose an activity to begin."),
        ]
        .spacing(10),
    )
    .padding(20)
    .width(Fill)
    .height(Fill)
    .style(panel_style(tokens, tokens.canvas));

    let context: Element<'_, Message> = if app.shell.context_visible {
        container(
            column![
                text("AI · WORKSPACE CONTEXT"),
                text("Current workspace"),
                text("4 capabilities enabled"),
                Space::new().height(Fill),
                button("Hide context").on_press(Message::ToggleContext),
            ]
            .spacing(10),
        )
        .padding(10)
        .width(Length::Fixed(250.0))
        .style(panel_style(tokens, tokens.panel))
        .into()
    } else {
        container(Space::new()).width(Length::Shrink).into()
    };

    let body = row![activity_rail, explorer, primary, context].height(Fill);

    let drawer: Element<'_, Message> = if app.shell.drawer_visible {
        container(
            row![
                text("TERMINAL  ·  local shell foundation"),
                Space::new().width(Fill),
                button("Close").on_press(Message::ToggleDrawer),
            ]
            .spacing(8),
        )
        .padding(10)
        .height(Length::Fixed(130.0))
        .style(panel_style(tokens, tokens.terminal_background))
        .into()
    } else {
        button("Open terminal drawer")
            .on_press(Message::ToggleDrawer)
            .width(Fill)
            .into()
    };

    container(column![header, body, drawer].height(Fill))
        .width(Fill)
        .height(Fill)
        .style(panel_style(tokens, tokens.canvas))
        .into()
}
```

- [x] **Step 3: Wire the native application**

Replace `crates/strukt-app/src/main.rs`:

```rust
#![forbid(unsafe_code)]

mod app;
mod view;

use app::StruktApp;

fn main() -> iced::Result {
    iced::application(StruktApp::default, StruktApp::update, view::view)
        .title("strukt")
        .subscription(StruktApp::subscription)
        .theme(StruktApp::theme)
        .run()
}
```

- [x] **Step 4: Compile the application**

Run:

```bash
cargo check -p strukt-app
cargo test --workspace
```

Expected: the application compiles and all domain tests pass.

- [x] **Step 5: Run and inspect the native window**

Run:

```bash
cargo run -p strukt-app
```

Verify manually:

- the window opens without a browser or Electron process;
- the activity rail, explorer, primary canvas, context panel, and drawer are visible;
- Files reopens the explorer after it is closed;
- Context toggles independently;
- the terminal drawer toggles;
- Command+B toggles the explorer on macOS and Control+B does so on Windows/Linux;
- Command+J toggles the drawer on macOS and Control+J does so on Windows/Linux;
- Command+\ toggles context on macOS and Control+\ does so on Windows/Linux;
- light/dark theme switching redraws the window;
- resizing the window keeps the primary canvas usable.

Record a screenshot and startup/idle measurements in the implementation PR.

- [x] **Step 6: Run static verification**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both commands exit successfully with no diagnostics.

- [x] **Step 7: Commit**

```bash
git add crates/strukt-app
git commit -m "feat: render native workspace shell"
```

### Task 6: Add cross-platform CI and close the milestone gate

**Files:**

- Create: `.github/workflows/ci.yml`
- Modify: `README.md`
- Modify: `docs/decisions/0001-native-ui-framework.md`
- Modify: `docs/tracker.md`

- [x] **Step 1: Add the cross-platform workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read

jobs:
  verify:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-14, windows-2022, ubuntu-24.04]
    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: 1.97.1
          components: clippy,rustfmt
      - uses: Swatinem/rust-cache@v2
      - name: Format
        run: cargo fmt --all --check
      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Test
        run: cargo test --workspace
      - name: Build native application
        run: cargo build -p strukt-app
```

- [x] **Step 2: Validate the workflow locally**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p strukt-app
```

Expected: every command exits successfully.

- [x] **Step 3: Update local development documentation**

Replace the `README.md` Local Development placeholder with:

````markdown
## Local Development

Install Rust through `rustup`; the repository pins the required toolchain.

```bash
cargo run -p strukt-app
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
````

Set the README status to `native shell foundation in progress`.

- [x] **Step 4: Update governance state**

Set the tracker row to:

```markdown
| Native shell foundation | In progress | `docs/specs/0001-workspace-shell-and-remote-development.md` | `docs/plans/0001-native-shell-foundation.md` | pending | pending | Iced validation milestone; local files and PTY follow in separate plans |
```

After macOS, Windows, and Linux CI all pass, macOS manual window checks are
recorded, and the Windows-native smoke gate in
[`0002-m1-windows-smoke-validation.md`](0002-m1-windows-smoke-validation.md)
passes, change ADR 0001 to `Accepted for the M1 foundation`.

- [x] **Step 5: Run the full verification gate**

```bash
test -f .forj-manifest.json
primary_repo="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
forj check "$primary_repo"
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p strukt-app
```

Expected: the worktree manifest exists, the shared repository's `forj` manifest is
printed, Git reports no whitespace errors, and all Cargo commands exit successfully.

`forj` currently requires `.git` to be a directory and therefore cannot inspect a
linked worktree directly. Resolving the shared primary checkout through
`git rev-parse --git-common-dir` preserves the governance check while the worktree's
own manifest is verified separately.

- [x] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml README.md docs/decisions/0001-native-ui-framework.md docs/tracker.md
git commit -m "ci: verify native shell across platforms"
```

## Plan completion criteria

This plan is complete only when:

- all focused tests were observed failing before implementation and passing after;
- the shell launches locally as a native Iced window;
- the accepted shell hierarchy is present and keyboard-accessible;
- light and dark tokens style every shell container surface;
- domain crates have no Iced dependency;
- formatting, Clippy, tests, and application build pass;
- macOS, Windows, and Linux CI jobs pass;
- ADR 0001 records measured validation results;
- the tracker and README match the implemented state.
