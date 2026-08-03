use std::str::FromStr;

use strukt_session::{
    CatalogError, MAX_SESSIONS, PaneId, PaneLifecycle, SessionCatalog, SessionId,
    SessionLayoutNode, WindowId,
};
use strukt_terminal::SplitAxis;

#[test]
fn opaque_ids_are_unique_exact_hex_and_string_serialized() {
    let first = SessionId::new().unwrap();
    let second = SessionId::new().unwrap();
    assert_ne!(first, second);
    assert_eq!(first.to_string().len(), 32);
    assert!(
        first
            .to_string()
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    assert_eq!(SessionId::from_str(&first.to_string()).unwrap(), first);
    assert!(SessionId::from_str("01").is_err());
    assert_eq!(serde_json::to_string(&first).unwrap().len(), 34);

    assert_ne!(PaneId::new().unwrap(), PaneId::new().unwrap());
    assert_ne!(WindowId::new().unwrap(), WindowId::new().unwrap());
}

#[test]
fn a_session_starts_with_one_window_and_one_stopped_pane() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = SessionCatalog::new();
    let session_id = catalog.create_session(0, " backend ", root.path()).unwrap();

    assert_eq!(catalog.revision(), 1);
    assert_eq!(catalog.active_session_id(), Some(session_id));
    let session = catalog.session(session_id).unwrap();
    assert_eq!(session.name(), "backend");
    assert_eq!(session.revision(), 1);
    assert_eq!(session.windows().len(), 1);
    let window = session.active_window().unwrap();
    assert_eq!(window.name(), "shell");
    assert_eq!(window.panes().len(), 1);
    assert_eq!(window.focused_pane().lifecycle(), &PaneLifecycle::Stopped);
    assert!(matches!(window.root(), SessionLayoutNode::Pane(_)));
}

#[test]
fn stale_revisions_and_invalid_names_fail_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = SessionCatalog::new();
    let session_id = catalog.create_session(0, "one", root.path()).unwrap();

    assert_eq!(
        catalog.rename_session(0, session_id, "two"),
        Err(CatalogError::StaleRevision {
            expected: 0,
            actual: 1
        })
    );
    assert_eq!(catalog.session(session_id).unwrap().name(), "one");
    assert_eq!(
        catalog.rename_session(1, session_id, "   "),
        Err(CatalogError::InvalidName)
    );
    assert_eq!(catalog.revision(), 1);
}

#[test]
fn duplicate_copies_definitions_but_no_runtime_state() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = SessionCatalog::new();
    let original = catalog.create_session(0, "backend", root.path()).unwrap();
    let original_pane = catalog
        .session(original)
        .unwrap()
        .active_window()
        .unwrap()
        .focused_pane()
        .id();
    catalog
        .set_pane_lifecycle(1, original, original_pane, PaneLifecycle::Running)
        .unwrap();
    let duplicate = catalog.duplicate_session(2, original).unwrap();

    let source = catalog.session(original).unwrap();
    let copy = catalog.session(duplicate).unwrap();
    assert_ne!(source.id(), copy.id());
    assert_eq!(copy.name(), "backend copy");
    assert_eq!(copy.windows().len(), source.windows().len());
    assert_ne!(
        copy.active_window().unwrap().focused_pane().id(),
        source.active_window().unwrap().focused_pane().id()
    );
    assert!(
        copy.windows()
            .iter()
            .flat_map(strukt_session::SessionWindow::panes)
            .all(|pane| pane.lifecycle() == &PaneLifecycle::Stopped && pane.generation() == 0)
    );
}

#[test]
fn duplicate_window_remaps_layout_into_stopped_definitions() {
    let directory = std::env::current_dir().expect("current directory");
    let mut catalog = SessionCatalog::new();
    let session = catalog
        .create_session(0, "source", &directory)
        .expect("session");
    let source = catalog
        .session(session)
        .expect("source")
        .active_window()
        .expect("window")
        .id();
    let duplicate = catalog
        .duplicate_window(catalog.revision(), session, source)
        .expect("duplicate window");
    let target = catalog.session(session).expect("session");
    assert_eq!(target.windows().len(), 2);
    let duplicate = target
        .windows()
        .iter()
        .find(|window| window.id() == duplicate)
        .expect("duplicate");
    assert_ne!(duplicate.id(), source);
    assert!(
        duplicate
            .panes()
            .all(|pane| pane.generation() == 0 && pane.lifecycle() == &PaneLifecycle::Stopped)
    );
    assert!(duplicate.validate());
}

#[test]
fn split_focus_and_ratio_preserve_a_valid_window_tree() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = SessionCatalog::new();
    let session_id = catalog.create_session(0, "work", root.path()).unwrap();
    let first = catalog
        .session(session_id)
        .unwrap()
        .active_window()
        .unwrap()
        .focused_pane()
        .id();
    let second = catalog
        .split_focused(1, session_id, SplitAxis::Vertical)
        .unwrap();
    catalog
        .set_focused_split_ratio(2, session_id, 6_000)
        .unwrap();
    catalog.focus_pane(3, session_id, first).unwrap();

    let window = catalog
        .session(session_id)
        .unwrap()
        .active_window()
        .unwrap();
    assert_eq!(window.focused_pane().id(), first);
    assert!(window.pane(second).is_some());
    assert!(window.validate());
}

#[test]
fn session_capacity_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = SessionCatalog::new();
    for index in 0..MAX_SESSIONS {
        catalog
            .create_session(catalog.revision(), format!("session-{index}"), root.path())
            .unwrap();
    }
    assert_eq!(
        catalog.create_session(catalog.revision(), "overflow", root.path()),
        Err(CatalogError::CapacityReached)
    );
}

#[test]
fn session_window_and_pane_actions_preserve_independent_hierarchies() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = SessionCatalog::new();
    let first_session = catalog.create_session(0, "first", root.path()).unwrap();
    let second_session = catalog.create_session(1, "second", root.path()).unwrap();
    catalog.activate_session(2, first_session).unwrap();

    let original_window = catalog
        .session(first_session)
        .unwrap()
        .active_window()
        .unwrap()
        .id();
    let second_window = catalog
        .create_window(3, first_session, " logs ", root.path())
        .unwrap();
    catalog
        .rename_window(4, first_session, second_window, "services")
        .unwrap();
    catalog
        .activate_window(5, first_session, original_window)
        .unwrap();
    let sibling = catalog
        .split_focused(6, first_session, SplitAxis::Horizontal)
        .unwrap();
    catalog.close_pane(7, first_session, sibling).unwrap();
    catalog
        .close_window(8, first_session, second_window)
        .unwrap();

    assert_eq!(catalog.active_session_id(), Some(first_session));
    assert_eq!(catalog.session(first_session).unwrap().windows().len(), 1);
    assert_eq!(catalog.session(second_session).unwrap().windows().len(), 1);
    assert!(
        catalog
            .session(first_session)
            .unwrap()
            .active_window()
            .unwrap()
            .validate()
    );
}

#[test]
fn live_panes_must_be_terminated_before_removal() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = SessionCatalog::new();
    let session = catalog.create_session(0, "live", root.path()).unwrap();
    let pane = catalog
        .session(session)
        .unwrap()
        .active_window()
        .unwrap()
        .focused_pane()
        .id();
    catalog
        .set_pane_lifecycle(1, session, pane, PaneLifecycle::Running)
        .unwrap();

    assert_eq!(
        catalog.remove_session(2, session),
        Err(CatalogError::SessionRunning)
    );
    assert_eq!(
        catalog.close_pane(2, session, pane),
        Err(CatalogError::PaneRunning)
    );
    assert_eq!(catalog.revision(), 2);
}
