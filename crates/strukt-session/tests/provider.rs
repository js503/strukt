use strukt_session::{
    AttentionState, PaneLifecycle, PaneScreenSnapshot, ProviderAction, ProviderCapabilities,
    ProviderCatalogSnapshot, ProviderError, ProviderHealth, ProviderKind, ServiceInstanceId,
    SessionCatalog, SnapshotError,
};
use strukt_terminal::{GridSize, TerminalModel};

#[test]
fn native_capabilities_gate_every_normalized_action() {
    let native = ProviderCapabilities::native_local();
    for action in [
        ProviderAction::Catalog,
        ProviderAction::Attach,
        ProviderAction::Detach,
        ProviderAction::CreateSession,
        ProviderAction::RenameSession,
        ProviderAction::DuplicateSession,
        ProviderAction::TerminateSession,
        ProviderAction::MutateWindows,
        ProviderAction::MutatePanes,
        ProviderAction::StructuredHistory,
        ProviderAction::Input,
        ProviderAction::Resize,
    ] {
        assert!(native.supports(action), "missing native action {action:?}");
    }

    let read_only = ProviderCapabilities::read_only();
    assert!(read_only.supports(ProviderAction::Catalog));
    assert!(read_only.supports(ProviderAction::Attach));
    assert!(!read_only.supports(ProviderAction::Input));
    assert!(!read_only.supports(ProviderAction::TerminateSession));
    assert_eq!(ProviderKind::NativeLocal.to_string(), "native-local");
}

#[test]
fn provider_failures_are_owned_bounded_and_redacted_by_construction() {
    let message = "x".repeat(4_096);
    let error = ProviderError::internal(message);
    let ProviderError::Internal { detail } = error else {
        panic!("unexpected provider error")
    };
    assert!(detail.len() <= 1_024);
    assert!(!detail.contains('\0'));

    assert_eq!(
        ProviderHealth::stale("transport lost"),
        ProviderHealth::Stale {
            detail: "transport lost".to_owned()
        }
    );
}

#[test]
fn terminal_snapshots_are_owned_serializable_and_bounded() {
    let size = GridSize::new(3, 8).unwrap();
    let mut model = TerminalModel::new(size, 16);
    model.advance(b"hello\r\nworld\x1b]2;service logs\x07");
    let terminal = model.snapshot(0);
    let snapshot = PaneScreenSnapshot::from_terminal(
        &terminal,
        41,
        3,
        PaneLifecycle::Running,
        7,
        AttentionState::Unread,
    )
    .unwrap();

    assert_eq!(snapshot.output_revision(), 41);
    assert_eq!(snapshot.generation(), 3);
    assert_eq!(snapshot.unread_count(), 7);
    assert_eq!(snapshot.attention(), AttentionState::Unread);
    assert_eq!(snapshot.title(), Some("service logs"));
    assert_eq!(snapshot.rows()[0][0].text(), "h");
    let json = serde_json::to_string(&snapshot).unwrap();
    for forbidden in ["process", "command", "environment", "secret", "input"] {
        assert!(!json.contains(forbidden), "snapshot leaked {forbidden}");
    }
}

#[test]
fn pathological_terminal_dimensions_are_rejected() {
    let size = GridSize::new(2_049, 1).unwrap();
    let model = TerminalModel::new(size, 0);
    assert_eq!(
        PaneScreenSnapshot::from_terminal(
            &model.snapshot(0),
            0,
            0,
            PaneLifecycle::Stopped,
            0,
            AttentionState::None,
        ),
        Err(SnapshotError::TooManyRows)
    );
}

#[test]
fn attention_and_unread_transitions_are_explicit() {
    assert_eq!(
        AttentionState::None.on_output(false),
        AttentionState::Unread
    );
    assert_eq!(AttentionState::None.on_output(true), AttentionState::None);
    assert_eq!(AttentionState::Unread.on_bell(), AttentionState::Attention);
    assert_eq!(AttentionState::Attention.on_viewed(), AttentionState::None);
}

#[test]
fn provider_catalog_snapshots_are_immutable_owned_values() {
    let instance = ServiceInstanceId::new().unwrap();
    let pane = strukt_session::PaneId::new().unwrap();
    let snapshot = ProviderCatalogSnapshot::new(
        instance,
        ProviderKind::NativeLocal,
        ProviderCapabilities::native_local(),
        SessionCatalog::new(),
    )
    .with_pane_statuses([(pane, 3, AttentionState::Attention)]);
    assert_eq!(snapshot.service_instance(), instance);
    assert_eq!(snapshot.catalog().revision(), 0);
    assert_eq!(snapshot.pane_status(pane), (3, AttentionState::Attention));
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(json.contains("native-local") || json.contains("NativeLocal"));
    for forbidden in ["process_handle", "command_history", "environment", "secret"] {
        assert!(!json.contains(forbidden));
    }
}
