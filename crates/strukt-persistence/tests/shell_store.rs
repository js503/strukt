use std::collections::BTreeSet;

use strukt_persistence::{
    PersistedCanvasLayout, SHELL_CONTRIBUTION_ID, SHELL_SCHEMA_VERSION, ShellSnapshotV1,
    ShellStoreError, WorkspaceSnapshot, WorkspaceStore, set_shell_contribution, shell_contribution,
};
use strukt_shell::{CanvasLayout, ShellAction, ShellState, SurfaceId};
use strukt_workspace::{WorkspaceRoot, WorkspaceState};
use tempfile::tempdir;

fn surface(id: &str) -> SurfaceId {
    SurfaceId::new(id).expect("valid surface id")
}

fn available() -> BTreeSet<SurfaceId> {
    [
        surface("files"),
        surface("files.sidebar"),
        surface("sessions"),
        surface("sessions.sidebar"),
        surface("editor.supporting"),
        surface("terminal.local.primary"),
        surface("problems"),
    ]
    .into_iter()
    .collect()
}

#[test]
fn shell_snapshot_round_trips_ids_and_bounded_geometry_only() {
    let mut state = ShellState::default();
    state.sidebar.width = 400;
    state.context.width = 480;
    state.drawer.height = 360;
    state.reduced_motion = true;
    state.apply(ShellAction::OpenDrawer(surface("problems")));
    state.apply(ShellAction::PromoteDrawerToSplit { ratio: 0.65 });

    let snapshot = ShellSnapshotV1::from_state(&state);
    let encoded = serde_json::to_string(&snapshot).expect("serialize shell snapshot");
    for forbidden in ["output", "command", "environment", "credential", "content"] {
        assert!(!encoded.contains(forbidden));
    }

    let restored = snapshot
        .restore(&available())
        .expect("restore shell snapshot");
    assert_eq!(restored.sidebar.width, 400);
    assert_eq!(restored.context.width, 480);
    assert_eq!(restored.drawer.height, 360);
    assert!(restored.reduced_motion);
    assert_eq!(restored.canvas, state.canvas);
}

#[test]
fn restoration_clamps_geometry_and_falls_back_from_missing_surfaces() {
    let mut snapshot = ShellSnapshotV1::from_state(&ShellState::default());
    snapshot.sidebar_width = u16::MAX;
    snapshot.context_width = 1;
    snapshot.canvas = PersistedCanvasLayout::Split {
        primary: "missing".to_owned(),
        secondary: "also-missing".to_owned(),
        ratio: f32::NAN,
    };
    snapshot.drawer_surface = Some("missing-drawer".to_owned());
    snapshot.drawer_visible = true;
    snapshot.drawer_height = u16::MAX;

    let restored = snapshot
        .restore(&available())
        .expect("restore with fallback");
    assert_eq!(restored.sidebar.width, 640);
    assert_eq!(restored.context.width, 180);
    assert_eq!(restored.drawer.height, 720);
    assert_eq!(
        restored.canvas,
        CanvasLayout::Single {
            primary: surface("sessions"),
        }
    );
    assert!(restored.drawer.surface.is_none());
    assert!(!restored.drawer.visible);
}

#[test]
fn legacy_prototype_defaults_migrate_to_quiet_precision_geometry() {
    let mut snapshot = ShellSnapshotV1::from_state(&ShellState::default());
    snapshot.schema_version = 1;
    snapshot.sidebar_width = 256;
    snapshot.context_width = 320;
    snapshot.drawer_height = 280;

    let restored = snapshot
        .restore(&available())
        .expect("restore legacy shell");

    assert_eq!(restored.sidebar.width, 218);
    assert_eq!(restored.context.width, 235);
    assert_eq!(restored.drawer.height, 205);
}

#[test]
fn pre_v4_shell_preserves_explicit_composition() {
    let mut snapshot = ShellSnapshotV1::from_state(&ShellState::default());
    snapshot.schema_version = 3;
    snapshot.active_activity = "search".to_owned();
    snapshot.canvas = PersistedCanvasLayout::Single {
        primary: "search".to_owned(),
    };
    snapshot.drawer_surface = Some("terminal.local.primary".to_owned());
    snapshot.drawer_visible = true;
    let mut encoded = serde_json::to_value(&snapshot).expect("encode pre-v4 snapshot");
    encoded
        .as_object_mut()
        .expect("snapshot object")
        .remove("reduced_motion");
    let snapshot: ShellSnapshotV1 =
        serde_json::from_value(encoded).expect("decode legacy snapshot");

    let restored = snapshot.restore(&available()).expect("restore M5.5 shell");

    assert_eq!(restored.active_activity, strukt_shell::Activity::Search);
    assert_eq!(
        restored.canvas,
        CanvasLayout::Single {
            primary: surface("search"),
        }
    );
    assert!(restored.drawer.visible);
    assert!(restored.reduced_motion);
    assert_eq!(
        restored.drawer.surface.as_ref().map(SurfaceId::as_str),
        Some("terminal.local.primary")
    );
}

#[test]
fn unsupported_or_malformed_shell_contributions_are_rejected() {
    let mut unsupported = ShellSnapshotV1::from_state(&ShellState::default());
    unsupported.schema_version = SHELL_SCHEMA_VERSION + 1;
    assert!(matches!(
        unsupported.restore(&available()),
        Err(ShellStoreError::UnsupportedSchema(_))
    ));

    let project = tempdir().expect("project");
    let root = WorkspaceRoot::open(project.path()).expect("root");
    let mut state = WorkspaceState::new(root);
    state.contributions.insert(
        SHELL_CONTRIBUTION_ID.to_owned(),
        serde_json::json!({"schema_version": 1, "active_activity": []}),
    );
    assert!(matches!(
        shell_contribution(&state),
        Err(ShellStoreError::MalformedContribution(_))
    ));
}

#[test]
fn shell_updates_preserve_opaque_sibling_contributions() {
    let project = tempdir().expect("project");
    let root = WorkspaceRoot::open(project.path()).expect("root");
    let mut state = WorkspaceState::new(root);
    state.contributions.insert(
        "future.plugin".to_owned(),
        serde_json::json!({"opaque": [1, 2, 3]}),
    );
    let snapshot = ShellSnapshotV1::from_state(&ShellState::default());

    set_shell_contribution(&mut state, &snapshot).expect("set shell contribution");

    assert_eq!(
        shell_contribution(&state).expect("decode shell"),
        Some(snapshot)
    );
    assert_eq!(
        state.contributions["future.plugin"],
        serde_json::json!({"opaque": [1, 2, 3]})
    );
}

#[test]
fn corrupt_shell_contribution_falls_back_to_last_valid_workspace() {
    let app_data = tempdir().expect("app data");
    let project = tempdir().expect("project");
    let root = WorkspaceRoot::open(project.path()).expect("root");
    let mut original = WorkspaceState::new(root);
    set_shell_contribution(
        &mut original,
        &ShellSnapshotV1::from_state(&ShellState::default()),
    )
    .expect("set shell");
    let store = WorkspaceStore::at(app_data.path());
    store.save(&original).expect("save original");

    let mut changed = original.clone();
    changed.explorer.show_hidden = true;
    store.save(&changed).expect("save changed");
    changed.contributions.insert(
        SHELL_CONTRIBUTION_ID.to_owned(),
        serde_json::json!({"schema_version": 99}),
    );
    let current = WorkspaceSnapshot {
        schema_version: 1,
        state: changed,
    };
    std::fs::write(
        store.current_path(original.root.id()),
        serde_json::to_vec(&current).expect("serialize current"),
    )
    .expect("write corrupt current");

    assert_eq!(
        store
            .load(original.root.id())
            .expect("load")
            .expect("snapshot")
            .state,
        original
    );
}
