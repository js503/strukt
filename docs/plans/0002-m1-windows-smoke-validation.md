# M1 Windows Native Smoke Validation Implementation Plan

- Status: In progress

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a deterministic native Windows startup gate that exercises the real
Iced application and verifies Windows command shortcuts without requiring Windows
hardware.

**Architecture:** `strukt-app` gains an explicit launch-mode boundary. Interactive
mode remains unchanged; smoke mode adds a three-second Iced timer whose first tick
prints a stable marker and requests a clean runtime exit. The existing Windows
GitHub Actions job runs the executable and requires both status zero and the marker.

**Tech Stack:** Rust 1.97.1, Iced 0.14, Cargo tests, GitHub Actions, PowerShell.

---

## Scope boundary

Included:

- exact `--smoke-test` launch-mode parsing
- three-second runtime-owned smoke timer
- clean `iced::exit()` lifecycle
- platform-command shortcut tests
- Windows-native CI smoke launch with output assertion and timeout
- M1 validation, ADR, roadmap, tracker, issue, and PR evidence updates

Excluded:

- screenshot or pixel comparison
- Windows packaging or installation
- synthetic UI automation
- manual Windows visual QA
- accessibility and IME validation

## File map

```text
crates/strukt-app/src/app.rs
crates/strukt-app/src/main.rs
.github/workflows/ci.yml
docs/specs/0002-m1-windows-smoke-validation.md
docs/plans/0002-m1-windows-smoke-validation.md
docs/evidence/m1-native-shell-validation.md
docs/decisions/0001-native-ui-framework.md
docs/roadmap.md
docs/tracker.md
```

- `app.rs` owns launch mode, lifecycle messages, shell updates, and subscriptions.
- `main.rs` parses process arguments and boots the selected launch mode.
- `ci.yml` owns the Windows process and success-marker assertion.
- Documentation records evidence limits and milestone state.

### Task 1: Define launch mode and shortcut behavior with tests

**Files:**

- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/main.rs`

- [ ] **Step 1: Write failing launch-mode tests**

Add these imports and tests to the existing `tests` module in
`crates/strukt-app/src/main.rs`:

```rust
use std::time::Duration;

use iced::keyboard::{self, Key, Location, Modifiers, key};

use crate::app::{LaunchMode, Message, StruktApp};

fn key_pressed(character: &'static str, code: key::Code, modifiers: Modifiers) -> Message {
    let key = Key::Character(character.into());

    Message::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: key::Physical::Code(code),
        location: Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

#[test]
fn launch_mode_requires_the_exact_smoke_flag() {
    assert_eq!(
        LaunchMode::from_args(Vec::<String>::new()),
        LaunchMode::Interactive
    );
    assert_eq!(
        LaunchMode::from_args(["--smoke-test".to_owned()]),
        LaunchMode::SmokeTest
    );
    assert_eq!(
        LaunchMode::from_args(["--smoke-testing".to_owned()]),
        LaunchMode::Interactive
    );
}

#[test]
fn only_smoke_mode_has_a_runtime_timeout() {
    assert_eq!(LaunchMode::Interactive.smoke_timeout(), None);
    assert_eq!(
        LaunchMode::SmokeTest.smoke_timeout(),
        Some(Duration::from_secs(3))
    );
}

#[test]
fn platform_command_shortcuts_toggle_shell_panels() {
    let mut app = StruktApp::default();

    let _ = app.update(key_pressed("b", key::Code::KeyB, Modifiers::COMMAND));
    let _ = app.update(key_pressed("j", key::Code::KeyJ, Modifiers::COMMAND));
    let _ = app.update(key_pressed(
        "\\",
        key::Code::Backslash,
        Modifiers::COMMAND,
    ));

    assert!(!app.shell.explorer_visible);
    assert!(app.shell.drawer_visible);
    assert!(!app.shell.context_visible);
}

#[test]
fn unmodified_shortcut_keys_do_not_toggle_shell_panels() {
    let mut app = StruktApp::default();

    let _ = app.update(key_pressed("b", key::Code::KeyB, Modifiers::empty()));
    let _ = app.update(key_pressed("j", key::Code::KeyJ, Modifiers::empty()));
    let _ = app.update(key_pressed(
        "\\",
        key::Code::Backslash,
        Modifiers::empty(),
    ));

    assert!(app.shell.explorer_visible);
    assert!(!app.shell.drawer_visible);
    assert!(app.shell.context_visible);
}
```

Retain the existing capability and message-state tests.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
cargo test -p strukt-app
```

Expected: compilation fails because `LaunchMode`,
`LaunchMode::from_args`, and `LaunchMode::smoke_timeout` do not exist.

- [ ] **Step 3: Implement the minimal launch-mode boundary**

Add this near the imports in `crates/strukt-app/src/app.rs`:

```rust
use std::time::Duration;

const SMOKE_TEST_DURATION: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LaunchMode {
    #[default]
    Interactive,
    SmokeTest,
}

impl LaunchMode {
    #[must_use]
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        if args.into_iter().any(|argument| argument == "--smoke-test") {
            Self::SmokeTest
        } else {
            Self::Interactive
        }
    }

    #[must_use]
    pub const fn smoke_timeout(self) -> Option<Duration> {
        match self {
            Self::Interactive => None,
            Self::SmokeTest => Some(SMOKE_TEST_DURATION),
        }
    }
}
```

Do not add launch mode to `StruktApp` yet. Task 2 introduces the field and
constructor when the lifecycle consumes them, which keeps this step free of
dead-code warnings.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run:

```bash
cargo test -p strukt-app
```

Expected: all `strukt-app` tests pass. The shortcut tests are characterization
coverage for existing message behavior; the launch-mode tests provide the
red-green cycle for new behavior.

- [ ] **Step 5: Run static verification**

Run:

```bash
cargo fmt --all --check
cargo clippy -p strukt-app --all-targets -- -D warnings
```

Expected: both commands exit successfully.

- [ ] **Step 6: Commit**

```bash
git add crates/strukt-app/src/app.rs crates/strukt-app/src/main.rs
git commit -m "test: cover platform launch and shortcuts"
```

### Task 2: Add deterministic smoke lifecycle with TDD

**Files:**

- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/main.rs`

- [ ] **Step 1: Write the failing smoke-exit task test**

Add this test to `crates/strukt-app/src/main.rs`:

```rust
#[test]
fn smoke_timeout_requests_runtime_work() {
    let mut app = StruktApp::new(LaunchMode::SmokeTest);

    let task = app.update(Message::SmokeTimeout);

    assert_eq!(task.units(), 1);
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test -p strukt-app smoke_timeout_requests_runtime_work
```

Expected: compilation fails because `Message::SmokeTimeout` does not exist and
`StruktApp::new` and the runtime lifecycle do not exist.

- [ ] **Step 3: Implement the minimal timer and exit lifecycle**

Update imports in `crates/strukt-app/src/app.rs`:

```rust
use iced::keyboard::{self, Key};
use iced::{Subscription, Task, Theme, time};
```

Add the lifecycle message:

```rust
pub enum Message {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
    Keyboard(keyboard::Event),
    SmokeTimeout,
}
```

Add `launch_mode` to `StruktApp` and make `Default` delegate to `new`:

```rust
#[derive(Debug)]
pub struct StruktApp {
    pub capabilities: CapabilityRegistry,
    pub shell: ShellState,
    launch_mode: LaunchMode,
}

impl Default for StruktApp {
    fn default() -> Self {
        Self::new(LaunchMode::Interactive)
    }
}

impl StruktApp {
    #[must_use]
    pub fn new(launch_mode: LaunchMode) -> Self {
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
            launch_mode,
        }
    }
}
```

Change `update` to return a task:

```rust
pub fn update(&mut self, message: Message) -> Task<Message> {
    let action = match message {
        Message::SelectActivity(activity) => Some(ShellAction::SelectActivity(activity)),
        Message::ToggleContext => Some(ShellAction::ToggleContext),
        Message::ToggleDrawer => Some(ShellAction::ToggleDrawer),
        Message::ToggleExplorer => Some(ShellAction::ToggleExplorer),
        Message::ToggleTheme => Some(ShellAction::ToggleTheme),
        Message::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
            if modifiers.command() =>
        {
            match key.as_ref() {
                Key::Character("b") => Some(ShellAction::ToggleExplorer),
                Key::Character("j") => Some(ShellAction::ToggleDrawer),
                Key::Character("\\") => Some(ShellAction::ToggleContext),
                _ => None,
            }
        }
        Message::Keyboard(_) => None,
        Message::SmokeTimeout => {
            println!("strukt smoke test: native event loop started");
            return iced::exit();
        }
    };

    if let Some(action) = action {
        self.shell.apply(action);
    }

    Task::none()
}
```

Update every existing test call that ignores `update` to explicitly consume the
returned task:

```rust
let _ = app.update(Message::ToggleExplorer);
let _ = app.update(Message::SelectActivity(Activity::Files));
let _ = app.update(Message::ToggleContext);
let _ = app.update(Message::ToggleDrawer);
```

Replace the static subscription with a state-aware method:

```rust
pub fn subscription(&self) -> Subscription<Message> {
    let keyboard = keyboard::listen().map(Message::Keyboard);

    match self.launch_mode.smoke_timeout() {
        Some(timeout) => Subscription::batch([
            keyboard,
            time::every(timeout).map(|_| Message::SmokeTimeout),
        ]),
        None => keyboard,
    }
}
```

Update `crates/strukt-app/src/main.rs`:

```rust
use app::{LaunchMode, StruktApp};

fn main() -> iced::Result {
    let launch_mode = LaunchMode::from_args(std::env::args().skip(1));

    iced::application(
        move || StruktApp::new(launch_mode),
        StruktApp::update,
        view::view,
    )
    .title("strukt")
    .subscription(StruktApp::subscription)
    .theme(StruktApp::theme)
    .run()
}
```

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run:

```bash
cargo test -p strukt-app
```

Expected: all `strukt-app` tests pass, including the smoke-exit task test.

- [ ] **Step 5: Run the real smoke mode locally**

Run:

```bash
cargo run -p strukt-app -- --smoke-test
```

Expected: a native window starts, the process prints
`strukt smoke test: native event loop started` after approximately three seconds,
and exits with status zero without user input.

- [ ] **Step 6: Verify interactive mode remains open**

Run:

```bash
cargo run -p strukt-app
```

Expected: the application stays open beyond three seconds. Close the window
normally after observing that it remains interactive.

- [ ] **Step 7: Run static verification**

Run:

```bash
cargo fmt --all --check
cargo clippy -p strukt-app --all-targets -- -D warnings
cargo test -p strukt-app
```

Expected: all commands exit successfully.

- [ ] **Step 8: Commit**

```bash
git add crates/strukt-app/src/app.rs crates/strukt-app/src/main.rs
git commit -m "feat: add native startup smoke mode"
```

### Task 3: Enforce the Windows smoke gate in CI

**Files:**

- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add the Windows-only smoke step**

Append this step after `Build native application`:

```yaml
      - name: Smoke launch native Windows application
        if: runner.os == 'Windows'
        shell: pwsh
        timeout-minutes: 2
        run: |
          $output = & cargo run -p strukt-app -- --smoke-test 2>&1
          $output | Write-Output

          if ($LASTEXITCODE -ne 0) {
            throw "strukt smoke test exited with code $LASTEXITCODE"
          }

          if (($output -join "`n") -notmatch "strukt smoke test: native event loop started") {
            throw "strukt smoke test did not emit its success marker"
          }
```

- [ ] **Step 2: Validate the workflow structure and local commands**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p strukt-app
git diff --check
```

Expected: formatting, Clippy, all tests, the native build, and whitespace checks
pass.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: smoke test native Windows startup"
```

### Task 4: Update evidence and milestone policy

**Files:**

- Modify: `docs/evidence/m1-native-shell-validation.md`
- Modify: `docs/decisions/0001-native-ui-framework.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`
- Modify: `docs/specs/0002-m1-windows-smoke-validation.md`
- Modify: `docs/plans/0002-m1-windows-smoke-validation.md`

- [ ] **Step 1: Record the validation policy before hosted results**

Update the evidence and ADR to state:

```markdown
M1 uses a deterministic Windows-native startup smoke mode because the project does
not currently have a human-operated Windows environment. The hosted gate exercises
the real Iced executable, native window and renderer initialization, event loop,
clean runtime exit, and Windows-native shortcut tests. It is not visual QA.

Human Windows visual, accessibility, IME, packaging, and installation validation
remain mandatory before M9 public-alpha readiness can be marked complete.
```

Keep ADR 0001 proposed and M1 in progress until the new hosted matrix passes.

- [ ] **Step 2: Run governance and documentation verification**

Run:

```bash
primary_repo="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
forj check "$primary_repo"
git diff --check
```

Expected: the forj manifest is found and no whitespace errors are reported.

- [ ] **Step 3: Commit the pre-CI governance update**

```bash
git add docs/evidence/m1-native-shell-validation.md \
  docs/decisions/0001-native-ui-framework.md \
  docs/roadmap.md \
  docs/tracker.md \
  docs/specs/0002-m1-windows-smoke-validation.md \
  docs/plans/0002-m1-windows-smoke-validation.md
git commit -m "docs: define automated Windows validation gate"
```

- [ ] **Step 4: Push and monitor hosted CI**

Run:

```bash
git push
gh pr checks 2 --repo js503/strukt --watch --interval 10
```

Expected: macOS 14, Ubuntu 24.04, and Windows 2022 pass. The Windows log includes
`strukt smoke test: native event loop started`.

- [ ] **Step 5: Record terminal hosted evidence**

After the final run passes:

- add the run URL and job durations to
  `docs/evidence/m1-native-shell-validation.md`;
- change ADR 0001 to `Accepted for the M1 foundation`;
- mark M1 `Complete` in `docs/roadmap.md` and `docs/tracker.md`;
- keep the human Windows QA requirement under M9;
- mark this spec and plan complete.

- [ ] **Step 6: Run the final local gate**

Run:

```bash
primary_repo="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
forj check "$primary_repo"
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p strukt-app
cargo check -p strukt-app --target x86_64-pc-windows-msvc
cargo check -p strukt-app --target x86_64-unknown-linux-gnu
```

Expected: governance, whitespace, formatting, Clippy, all tests, native build, and
both cross-target checks pass.

- [ ] **Step 7: Commit and push final evidence**

```bash
git add docs/evidence/m1-native-shell-validation.md \
  docs/decisions/0001-native-ui-framework.md \
  docs/roadmap.md \
  docs/tracker.md \
  docs/specs/0002-m1-windows-smoke-validation.md \
  docs/plans/0002-m1-windows-smoke-validation.md
git commit -m "docs: complete milestone one validation"
git push
```

- [ ] **Step 8: Verify the final PR head**

Run:

```bash
gh pr checks 2 --repo js503/strukt --watch --interval 10
git status --short --branch
```

Expected: all three hosted jobs pass again after the evidence commit, and the
feature worktree is clean and synchronized with its upstream.

## Completion criteria

This plan is complete only when:

- the launch-mode test was observed failing before implementation and passing after;
- smoke timeout returns a runtime task and the real application exits cleanly;
- interactive mode remains open beyond the smoke interval;
- platform-command shortcut tests run on Windows;
- Windows CI requires the success marker and status zero;
- the final three-platform matrix passes on the final PR head;
- evidence distinguishes automated startup validation from human Windows QA;
- ADR 0001, the roadmap, tracker, issue, and PR reflect the verified state.
