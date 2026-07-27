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
        };
        if let Some(action) = action {
            self.shell.apply(action);
        }
    }

    #[must_use]
    pub fn theme(&self) -> Theme {
        match self.shell.theme_mode {
            ThemeMode::Light => Theme::Light,
            ThemeMode::Dark => Theme::Dark,
        }
    }

    pub fn subscription() -> Subscription<Message> {
        keyboard::listen().map(Message::Keyboard)
    }
}
