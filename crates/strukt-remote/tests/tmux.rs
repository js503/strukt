use std::path::PathBuf;
use std::process::Command;

use strukt_remote::{
    PersistentProvider, SessionPayload, TmuxControlEvent, TmuxManager, TmuxProvider, TmuxRequest,
    TmuxResponse, TmuxTarget, read_frame, write_frame,
};

#[test]
fn discovery_uses_fixed_argv_and_parses_machine_records() {
    let provider = TmuxProvider::new(PathBuf::from("/usr/bin/tmux")).unwrap();
    let spec = provider.discovery_command();
    assert_eq!(spec.program(), PathBuf::from("/usr/bin/tmux"));
    assert_eq!(
        spec.arguments(),
        [
            "list-panes",
            "-a",
            "-F",
            "#{session_id}\x1f#{session_name}\x1f#{window_id}\x1f#{window_name}\x1f#{pane_id}\x1f#{pane_active}\x1f#{pane_width}\x1f#{pane_height}",
        ]
    );

    let output = b"$2\\037api;$(touch nope)\\037@3\\037editor\\037%4\\0371\\037120\\03740\n\
$2\\037api;$(touch nope)\\037@3\\037editor\\037%5\\0370\\03780\\03724\n";
    let catalog = provider.parse_discovery(output).unwrap();
    assert_eq!(catalog.sessions().len(), 1);
    assert_eq!(catalog.sessions()[0].raw_id(), "$2");
    assert_eq!(catalog.sessions()[0].name(), "api;$(touch nope)");
    assert_eq!(catalog.sessions()[0].windows()[0].panes().len(), 2);
}

#[test]
fn hostile_or_oversized_discovery_is_rejected() {
    let provider = TmuxProvider::new(PathBuf::from("/usr/bin/tmux")).unwrap();
    assert!(
        provider
            .parse_discovery(b"$(id)\\037name\\037@1\\037win\\037%1\\0371\\03780\\03724\n")
            .is_err()
    );
    assert!(
        provider
            .parse_discovery(&vec![b'x'; 1024 * 1024 + 1])
            .is_err()
    );
}

#[test]
fn attach_input_resize_and_capture_never_use_a_shell() {
    let provider = TmuxProvider::new(PathBuf::from("/usr/bin/tmux")).unwrap();
    let session = TmuxTarget::session("$12").unwrap();
    let pane = TmuxTarget::pane("%7").unwrap();
    assert_eq!(
        provider.attach_command(&session).arguments(),
        ["-C", "attach-session", "-t", "$12"]
    );
    assert_eq!(
        provider.input_command(&pane, b"hi\n").unwrap().arguments(),
        ["send-keys", "-t", "%7", "-H", "68", "69", "0a"]
    );
    assert_eq!(
        provider.resize_command(&pane, 40, 120).unwrap().arguments(),
        ["resize-pane", "-t", "%7", "-x", "120", "-y", "40"]
    );
    assert_eq!(
        provider.capture_command(&pane).arguments(),
        ["capture-pane", "-p", "-e", "-S", "-1000", "-t", "%7"]
    );
}

#[test]
fn control_mode_records_are_bounded_and_decode_octal_output() {
    assert_eq!(
        TmuxControlEvent::parse(b"%output %7 hello\\040world\\012").unwrap(),
        TmuxControlEvent::Output {
            pane: "%7".into(),
            bytes: b"hello world\n".to_vec(),
        }
    );
    assert_eq!(
        TmuxControlEvent::parse(b"%session-closed $2").unwrap(),
        TmuxControlEvent::SessionClosed {
            session: "$2".into()
        }
    );
    assert!(TmuxControlEvent::parse(&vec![b'x'; 64 * 1024 + 1]).is_err());
    assert!(TmuxControlEvent::parse(b"%output nope bytes").is_err());
}

#[test]
fn real_tmux_discovery_uses_an_isolated_server_when_available() {
    let Some(executable) = TmuxProvider::discover_executable() else {
        return;
    };
    let server = format!("strukt-m5-test-{}", std::process::id());
    let status = Command::new(&executable)
        .args(["-L", &server, "new-session", "-d", "-s", "m5-real"])
        .status()
        .unwrap();
    assert!(status.success());
    let provider = TmuxProvider::new(executable.clone())
        .unwrap()
        .with_server_name(&server)
        .unwrap();
    let catalog = provider.discover().unwrap();
    let session = catalog
        .sessions()
        .iter()
        .find(|item| item.name() == "m5-real")
        .unwrap();
    let session_id = session.raw_id().to_owned();
    let pane_id = session.windows()[0].panes()[0].raw_id().to_owned();
    let manager = TmuxManager::new(provider);
    assert!(matches!(
        tmux_exchange(
            &manager,
            &TmuxRequest::Attach {
                session: session_id
            }
        ),
        TmuxResponse::Attached { .. }
    ));
    assert_eq!(
        tmux_exchange(
            &manager,
            &TmuxRequest::Input {
                pane: pane_id.clone(),
                bytes: b"echo M5_TMUX_MARKER\n".to_vec(),
            },
        ),
        TmuxResponse::Acknowledged
    );
    std::thread::sleep(std::time::Duration::from_millis(100));
    let TmuxResponse::Snapshot { bytes, .. } = tmux_exchange(
        &manager,
        &TmuxRequest::Snapshot {
            pane: pane_id.clone(),
        },
    ) else {
        panic!("expected tmux snapshot")
    };
    assert!(String::from_utf8_lossy(&bytes).contains("M5_TMUX_MARKER"));
    assert_eq!(
        tmux_exchange(&manager, &TmuxRequest::Detach),
        TmuxResponse::Detached
    );

    let cleanup = Command::new(executable)
        .args(["-L", &server, "kill-server"])
        .status()
        .unwrap();
    assert!(cleanup.success());
}

fn tmux_exchange(manager: &TmuxManager, request: &TmuxRequest) -> TmuxResponse {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &request, 1024 * 1024).unwrap();
    let payload = SessionPayload::new(PersistentProvider::Tmux, bytes).unwrap();
    let response = manager.exchange(&payload).unwrap();
    read_frame(&mut std::io::Cursor::new(response.bytes()), 1024 * 1024).unwrap()
}
