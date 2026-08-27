#![allow(
    dead_code,
    reason = "logical shortcut inventory is consumed incrementally by feature views"
)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalShortcut {
    Primary,
    CommandCenter,
    ActivityNavigation,
    ToggleSidebar,
    TerminalDrawer,
    ToggleContext,
    Escape,
}

impl LogicalShortcut {
    pub const fn text_alternative(self) -> &'static str {
        match self {
            Self::Primary => "Primary modifier",
            Self::CommandCenter => "Open command center",
            Self::ActivityNavigation => "Navigate activities",
            Self::ToggleSidebar => "Toggle contextual sidebar",
            Self::TerminalDrawer => "Focus terminal drawer",
            Self::ToggleContext => "Toggle workspace context",
            Self::Escape => "Close the topmost interface layer",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformShortcut {
    primary: &'static str,
}

impl PlatformShortcut {
    pub const fn for_target(target: &str) -> Self {
        Self {
            primary: if matches!(target.as_bytes(), b"macos") {
                "⌘"
            } else {
                "Ctrl+"
            },
        }
    }

    pub fn display(self, shortcut: LogicalShortcut) -> String {
        match shortcut {
            LogicalShortcut::Primary => self.primary.to_owned(),
            LogicalShortcut::CommandCenter => format!("{}K", self.primary),
            LogicalShortcut::ToggleSidebar => format!("{}B", self.primary),
            LogicalShortcut::TerminalDrawer => format!("{}J", self.primary),
            LogicalShortcut::ToggleContext => format!("{}\\", self.primary),
            LogicalShortcut::ActivityNavigation => format!("{}1…9", self.primary),
            LogicalShortcut::Escape => "Escape".to_owned(),
        }
    }
}

pub const fn current_platform() -> PlatformShortcut {
    PlatformShortcut::for_target(std::env::consts::OS)
}
