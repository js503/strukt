use strukt_persistence::{
    SESSION_CONTRIBUTION_ID, SessionMigrationOutcome, TERMINAL_CONTRIBUTION_ID,
    TerminalSessionSnapshot, apply_session_migration_metadata, plan_session_migration,
    session_contribution, set_terminal_contribution,
};
use strukt_session::{PaneLifecycle, SessionCatalog};
use strukt_terminal::{SplitAxis, TerminalWorkspace};
use strukt_workspace::{WorkspaceRoot, WorkspaceState};
use tempfile::tempdir;

#[test]
fn m2_tabs_migrate_to_one_stopped_local_session_with_exact_layouts() {
    let project = tempdir().unwrap();
    let mut state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let mut terminals = TerminalWorkspace::default();
    terminals.create_tab("shell", project.path()).unwrap();
    terminals.split_focused(SplitAxis::Vertical).unwrap();
    terminals.create_tab("logs", project.path()).unwrap();
    let terminal = TerminalSessionSnapshot::from_workspace(&terminals);
    set_terminal_contribution(&mut state, &terminal).unwrap();

    let SessionMigrationOutcome::Planned(plan) = plan_session_migration(&state, None).unwrap()
    else {
        panic!("migration expected");
    };
    let session = plan.catalog.sessions().next().expect("local session");
    assert_eq!(session.name(), "Local");
    assert_eq!(session.windows().len(), 2);
    assert_eq!(session.windows()[0].name(), "shell");
    assert_eq!(session.windows()[0].panes().len(), 2);
    assert_eq!(session.windows()[1].name(), "logs");
    assert!(
        session
            .windows()
            .iter()
            .flat_map(strukt_session::SessionWindow::panes)
            .all(|pane| pane.generation() == 0 && pane.lifecycle() == &PaneLifecycle::Stopped)
    );
    assert!(plan.catalog.validate().is_ok());
    assert!(!project.path().join(".strukt").exists());
}

#[test]
fn migration_is_idempotent_and_m3_wins_without_removing_legacy_data() {
    let project = tempdir().unwrap();
    let mut state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let mut terminals = TerminalWorkspace::default();
    terminals.create_tab("shell", project.path()).unwrap();
    set_terminal_contribution(
        &mut state,
        &TerminalSessionSnapshot::from_workspace(&terminals),
    )
    .unwrap();
    let mut existing = SessionCatalog::new();
    existing
        .create_session(0, "Existing", project.path())
        .unwrap();

    assert_eq!(
        plan_session_migration(&state, Some(&existing)).unwrap(),
        SessionMigrationOutcome::M3Authoritative
    );
    assert!(state.contributions.contains_key(TERMINAL_CONTRIBUTION_ID));
}

#[test]
fn metadata_commit_removes_only_legacy_terminal_after_service_save_boundary() {
    let project = tempdir().unwrap();
    let mut state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    state
        .contributions
        .insert("future.plugin".into(), serde_json::json!({"kept": true}));
    let mut terminals = TerminalWorkspace::default();
    terminals.create_tab("shell", project.path()).unwrap();
    set_terminal_contribution(
        &mut state,
        &TerminalSessionSnapshot::from_workspace(&terminals),
    )
    .unwrap();
    let SessionMigrationOutcome::Planned(plan) = plan_session_migration(&state, None).unwrap()
    else {
        panic!("migration expected");
    };

    assert!(state.contributions.contains_key(TERMINAL_CONTRIBUTION_ID));
    apply_session_migration_metadata(&mut state, &plan).unwrap();

    assert!(!state.contributions.contains_key(TERMINAL_CONTRIBUTION_ID));
    assert!(state.contributions.contains_key(SESSION_CONTRIBUTION_ID));
    assert_eq!(
        state.contributions["future.plugin"],
        serde_json::json!({"kept": true})
    );
    assert_eq!(
        session_contribution(&state).unwrap(),
        Some(plan.contribution)
    );
}
