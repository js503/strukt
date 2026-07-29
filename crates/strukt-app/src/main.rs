#![forbid(unsafe_code)]

mod app;
mod view;

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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iced::keyboard::{self, Key, Location, Modifiers, key};
    use strukt_core::CapabilityId;
    use strukt_shell::Activity;

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
    fn built_in_capabilities_are_registered() {
        let app = StruktApp::default();

        assert!(app.capabilities.is_enabled(CapabilityId::FILES));
        assert!(app.capabilities.is_enabled(CapabilityId::TERMINAL));
        assert!(app.capabilities.is_enabled(CapabilityId::AI));
    }

    #[test]
    fn application_messages_drive_shell_state() {
        let mut app = StruktApp::default();

        let _ = app.update(Message::ToggleExplorer);
        assert!(!app.shell.explorer_visible);

        let _ = app.update(Message::SelectActivity(Activity::Files));
        assert!(app.shell.explorer_visible);

        let _ = app.update(Message::ToggleContext);
        let _ = app.update(Message::ToggleDrawer);
        assert!(!app.shell.context_visible);
        assert!(app.shell.drawer_visible);
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
    fn smoke_timeout_requests_runtime_work() {
        let mut app = StruktApp::new(LaunchMode::SmokeTest);

        let task = app.update(Message::SmokeTimeout);

        assert_eq!(task.units(), 1);
    }

    #[test]
    fn platform_command_shortcuts_toggle_shell_panels() {
        let mut app = StruktApp::default();

        let _ = app.update(key_pressed("b", key::Code::KeyB, Modifiers::COMMAND));
        let _ = app.update(key_pressed("j", key::Code::KeyJ, Modifiers::COMMAND));
        let _ = app.update(key_pressed("\\", key::Code::Backslash, Modifiers::COMMAND));

        assert!(!app.shell.explorer_visible);
        assert!(app.shell.drawer_visible);
        assert!(!app.shell.context_visible);
    }

    #[test]
    fn unmodified_shortcut_keys_do_not_toggle_shell_panels() {
        let mut app = StruktApp::default();

        let _ = app.update(key_pressed("b", key::Code::KeyB, Modifiers::empty()));
        let _ = app.update(key_pressed("j", key::Code::KeyJ, Modifiers::empty()));
        let _ = app.update(key_pressed("\\", key::Code::Backslash, Modifiers::empty()));

        assert!(app.shell.explorer_visible);
        assert!(!app.shell.drawer_visible);
        assert!(app.shell.context_visible);
    }
}
