use std::time::Duration;

use strukt_remote::{
    ConnectionCapabilities, ConnectionMachine, ConnectionPhase, RecoveryAction, RetryPolicy,
};

#[test]
fn connection_progresses_through_terminal_fallback_to_ready() {
    let mut machine = ConnectionMachine::new(RetryPolicy::default());
    assert_eq!(machine.projection().phase, ConnectionPhase::Disconnected);
    assert_eq!(machine.generation(), 0);

    machine.connect().unwrap();
    assert_eq!(machine.projection().phase, ConnectionPhase::Connecting);
    assert_eq!(machine.generation(), 1);

    machine.terminal_available().unwrap();
    assert_eq!(machine.projection().phase, ConnectionPhase::TerminalOnly);
    assert_eq!(
        machine.projection().recovery,
        vec![RecoveryAction::InstallHelper]
    );

    machine.negotiate_helper().unwrap();
    assert_eq!(
        machine.projection().phase,
        ConnectionPhase::NegotiatingHelper
    );

    let capabilities = ConnectionCapabilities {
        files: true,
        search: true,
        git: true,
        processes: true,
        language: false,
        watches: true,
    };
    machine.helper_ready(capabilities).unwrap();
    assert_eq!(machine.projection().phase, ConnectionPhase::Ready);
    assert_eq!(machine.projection().capabilities, capabilities);
    assert_eq!(machine.projection().generation, 1);
}

#[test]
fn transport_loss_keeps_a_stale_snapshot_and_rejects_old_generation() {
    let mut machine = ConnectionMachine::new(RetryPolicy::default());
    machine.connect().unwrap();
    let first_generation = machine.generation();
    machine.terminal_available().unwrap();
    machine.helper_ready(ConnectionCapabilities::all()).unwrap();
    machine.transport_lost("network unavailable").unwrap();

    let stale = machine.projection();
    assert_eq!(stale.phase, ConnectionPhase::Stale);
    assert_eq!(stale.detail.as_deref(), Some("network unavailable"));
    assert_eq!(stale.capabilities, ConnectionCapabilities::all());
    assert_eq!(
        stale.recovery,
        vec![RecoveryAction::RetryNow, RecoveryAction::Disconnect]
    );

    let delay = machine.begin_retry().unwrap();
    assert_eq!(delay, Duration::from_millis(250));
    assert_eq!(machine.projection().phase, ConnectionPhase::Reconnecting);
    assert!(machine.accepts_generation(first_generation));
    machine.retry_connected().unwrap();
    assert_eq!(machine.generation(), first_generation + 1);
    assert!(!machine.accepts_generation(first_generation));
}

#[test]
fn retries_are_capped_and_explicit_disconnect_cancels_them() {
    let policy =
        RetryPolicy::new(Duration::from_millis(100), Duration::from_millis(350), 3).unwrap();
    let mut machine = ConnectionMachine::new(policy);
    machine.connect().unwrap();
    machine.terminal_available().unwrap();
    machine.transport_lost("lost").unwrap();

    assert_eq!(machine.begin_retry().unwrap(), Duration::from_millis(100));
    machine.retry_failed("still lost").unwrap();
    assert_eq!(machine.begin_retry().unwrap(), Duration::from_millis(200));
    machine.retry_failed("still lost").unwrap();
    assert_eq!(machine.begin_retry().unwrap(), Duration::from_millis(350));
    machine.retry_failed("still lost").unwrap();
    assert!(machine.begin_retry().is_err());
    assert_eq!(machine.projection().phase, ConnectionPhase::Failed);

    machine.disconnect().unwrap();
    assert_eq!(machine.projection().phase, ConnectionPhase::Disconnecting);
    machine.disconnected().unwrap();
    assert_eq!(machine.projection().phase, ConnectionPhase::Disconnected);
    assert_eq!(
        machine.projection().capabilities,
        ConnectionCapabilities::default()
    );
}

#[test]
fn invalid_transitions_do_not_mutate_state() {
    let mut machine = ConnectionMachine::new(RetryPolicy::default());
    let before = machine.projection();
    assert!(machine.helper_ready(ConnectionCapabilities::all()).is_err());
    assert!(machine.transport_lost("no transport").is_err());
    assert_eq!(machine.projection(), before);

    machine.connect().unwrap();
    assert!(machine.connect().is_err());
    assert!(machine.negotiate_helper().is_err());
    assert_eq!(machine.projection().phase, ConnectionPhase::Connecting);
}

#[test]
fn error_details_are_bounded_and_sanitized() {
    let mut machine = ConnectionMachine::new(RetryPolicy::default());
    machine.connect().unwrap();
    machine.fail(format!("bad\0{}", "x".repeat(4_096))).unwrap();
    let detail = machine.projection().detail.unwrap();
    assert!(!detail.contains('\0'));
    assert!(detail.len() <= 1_024);
}
