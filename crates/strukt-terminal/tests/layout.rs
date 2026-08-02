use std::path::PathBuf;

use strukt_terminal::{
    LayoutNode, PaneState, SplitAxis, TerminalWorkspace, TerminalWorkspaceError,
};

#[test]
fn split_close_and_focus_preserve_a_valid_tree() {
    let mut workspace = TerminalWorkspace::default();
    let first = workspace.create_tab("Terminal 1", "/workspace").unwrap();
    let second = workspace.split_focused(SplitAxis::Vertical).unwrap();
    assert_eq!(workspace.focused_pane(), Some(second));

    workspace.close_pane(second).unwrap();

    assert_eq!(workspace.focused_pane(), Some(first));
    assert!(workspace.active_tab().unwrap().root().is_pane());
}

#[test]
fn restored_panes_are_stopped_and_never_retain_commands() {
    let mut original = TerminalWorkspace::default();
    original.create_tab("build", "/workspace").unwrap();
    original.split_focused(SplitAxis::Horizontal).unwrap();
    let snapshot = original.snapshot();

    let restored = TerminalWorkspace::restore(snapshot).unwrap();

    assert!(
        restored
            .panes()
            .all(|pane| matches!(pane.state(), PaneState::Stopped))
    );
    assert!(restored.panes().all(|pane| pane.command().is_none()));
}

#[test]
fn tabs_have_independent_focus_names_and_lifecycle_state() {
    let mut workspace = TerminalWorkspace::default();
    let first = workspace.create_tab("shell", "/one").unwrap();
    let first_tab = workspace.active_tab().unwrap().id();
    let second = workspace.create_tab("tests", "/two").unwrap();
    let second_tab = workspace.active_tab().unwrap().id();
    workspace
        .set_pane_state(second, PaneState::Exited { code: Some(7) })
        .unwrap();

    workspace.activate_tab(first_tab).unwrap();
    assert_eq!(workspace.focused_pane(), Some(first));
    assert!(matches!(
        workspace.pane(first).unwrap().state(),
        PaneState::Stopped
    ));
    workspace.rename_tab(first_tab, "server").unwrap();
    assert_eq!(workspace.active_tab().unwrap().name(), "server");

    workspace.activate_tab(second_tab).unwrap();
    assert!(matches!(
        workspace.pane(second).unwrap().state(),
        PaneState::Exited { code: Some(7) }
    ));
}

#[test]
fn invalid_snapshots_and_layout_operations_are_rejected() {
    let mut workspace = TerminalWorkspace::default();
    workspace.create_tab("shell", "/workspace").unwrap();
    assert_eq!(
        workspace.rename_active_tab("   ").unwrap_err(),
        TerminalWorkspaceError::InvalidName
    );
    assert_eq!(
        workspace.set_focused_split_ratio(999).unwrap_err(),
        TerminalWorkspaceError::InvalidSplitRatio
    );
    assert_eq!(
        workspace.set_focused_split_ratio(5_000).unwrap_err(),
        TerminalWorkspaceError::NoFocusedSplit
    );

    let mut snapshot = workspace.snapshot();
    snapshot.tabs[0].focused_pane = strukt_terminal::TerminalPaneId::new();
    assert_eq!(
        TerminalWorkspace::restore(snapshot).unwrap_err(),
        TerminalWorkspaceError::InvalidFocusedPane
    );
}

#[test]
fn closing_the_last_pane_returns_to_an_empty_workspace() {
    let mut workspace = TerminalWorkspace::default();
    let pane = workspace
        .create_tab("shell", PathBuf::from("/workspace"))
        .unwrap();

    workspace.close_pane(pane).unwrap();

    assert!(workspace.tabs().is_empty());
    assert_eq!(workspace.focused_pane(), None);
}

#[test]
fn restore_rejects_duplicate_ids_bad_ratios_and_empty_directories() {
    let mut workspace = TerminalWorkspace::default();
    workspace.create_tab("shell", "/workspace").unwrap();
    workspace.split_focused(SplitAxis::Vertical).unwrap();
    let valid = workspace.snapshot();

    let mut duplicate = valid.clone();
    let duplicated_pane = duplicate.tabs[0].panes[0].clone();
    duplicate.tabs[0].panes.push(duplicated_pane);
    assert!(TerminalWorkspace::restore(duplicate).is_err());

    let mut bad_ratio = valid.clone();
    let LayoutNode::Split {
        ratio_basis_points, ..
    } = &mut bad_ratio.tabs[0].root
    else {
        panic!("fixture must contain a split");
    };
    *ratio_basis_points = 999;
    assert_eq!(
        TerminalWorkspace::restore(bad_ratio).unwrap_err(),
        TerminalWorkspaceError::InvalidSplitRatio
    );

    let mut empty_directory = valid;
    empty_directory.tabs[0].panes[0].working_directory = PathBuf::new();
    assert_eq!(
        TerminalWorkspace::restore(empty_directory).unwrap_err(),
        TerminalWorkspaceError::InvalidWorkingDirectory
    );
}

#[test]
fn focus_ratio_and_restart_transitions_are_validated() {
    let mut workspace = TerminalWorkspace::default();
    let first = workspace.create_tab("shell", "/workspace").unwrap();
    let second = workspace.split_focused(SplitAxis::Vertical).unwrap();
    workspace.focus_pane(first).unwrap();
    workspace.set_focused_split_ratio(6_000).unwrap();
    assert_eq!(workspace.focused_pane(), Some(first));

    workspace
        .set_pane_state(second, PaneState::Exited { code: Some(0) })
        .unwrap();
    workspace.restart_pane(second).unwrap();
    assert!(matches!(
        workspace.pane(second).unwrap().state(),
        PaneState::Starting
    ));
    workspace
        .set_pane_state(second, PaneState::Running)
        .unwrap();
    workspace.restart_pane(second).unwrap();
    assert_eq!(
        workspace.restart_pane(second).unwrap_err(),
        TerminalWorkspaceError::InvalidPaneTransition
    );
}
