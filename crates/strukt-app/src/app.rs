use std::time::Duration;

use iced::keyboard::{self, Key};
use iced::{Subscription, Task, Theme, time};
use strukt_core::{CapabilityDescriptor, CapabilityId, CapabilityRegistry};
use strukt_shell::{Activity, ShellAction, ShellState};
use strukt_theme::ThemeMode;

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

#[derive(Debug)]
pub struct StruktApp {
    pub capabilities: CapabilityRegistry,
    pub shell: ShellState,
    launch_mode: LaunchMode,
}

#[derive(Clone, Debug)]
pub enum Message {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
    Keyboard(keyboard::Event),
    SmokeTimeout,
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

impl StruktApp {
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

    #[must_use]
    pub fn theme(&self) -> Theme {
        match self.shell.theme_mode {
            ThemeMode::Light => Theme::Light,
            ThemeMode::Dark => Theme::Dark,
        }
    }

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
}
