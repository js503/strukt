#![forbid(unsafe_code)]

mod app;
mod view;

use app::StruktApp;

fn main() -> iced::Result {
    iced::application(StruktApp::default, StruktApp::update, view::view)
        .title("strukt")
        .subscription(|_| StruktApp::subscription())
        .theme(StruktApp::theme)
        .run()
}

#[cfg(test)]
mod tests {
    use strukt_core::CapabilityId;
    use strukt_shell::Activity;

    use crate::app::{Message, StruktApp};

    #[test]
    fn built_in_capabilities_are_registered() {
        let app = StruktApp::default();

        assert!(app.capabilities.is_enabled(CapabilityId::FILES));
        assert!(app.capabilities.is_enabled(CapabilityId::TERMINAL));
        assert!(app.capabilities.is_enabled(CapabilityId::AI));
    }

    #[test]
    fn application_messages_drive_shell_state() {
        let mut app = StruktApp::default();

        app.update(Message::ToggleExplorer);
        assert!(!app.shell.explorer_visible);

        app.update(Message::SelectActivity(Activity::Files));
        assert!(app.shell.explorer_visible);

        app.update(Message::ToggleContext);
        app.update(Message::ToggleDrawer);
        assert!(!app.shell.context_visible);
        assert!(app.shell.drawer_visible);
    }
}
