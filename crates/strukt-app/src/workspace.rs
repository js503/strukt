use std::path::PathBuf;

use strukt_fs::{DiscoveryOptions, DiscoveryReport, discover_report};
use strukt_persistence::WorkspaceStore;
use strukt_workspace::{WorkspaceRoot, WorkspaceState};

#[derive(Clone, Debug)]
pub struct OpenedWorkspace {
    pub state: WorkspaceState,
    pub discovery: DiscoveryReport,
}

pub(crate) fn open_workspace_with_store(
    path: PathBuf,
    store: &WorkspaceStore,
) -> Result<OpenedWorkspace, String> {
    let root = WorkspaceRoot::open(path).map_err(|error| error.to_string())?;
    let state = store
        .load_for_root(&root)
        .map_err(|error| error.to_string())?
        .map_or_else(
            || WorkspaceState::new(root.clone()),
            |snapshot| snapshot.state,
        );
    discover_workspace(state)
}

pub(crate) fn open_workspace_without_store(path: PathBuf) -> Result<OpenedWorkspace, String> {
    let root = WorkspaceRoot::open(path).map_err(|error| error.to_string())?;
    discover_workspace(WorkspaceState::new(root))
}

fn discover_workspace(state: WorkspaceState) -> Result<OpenedWorkspace, String> {
    let discovery = discover_report(
        state.root.path(),
        DiscoveryOptions {
            show_hidden: state.explorer.show_hidden,
            show_ignored: state.explorer.show_ignored,
            ..DiscoveryOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;

    Ok(OpenedWorkspace { state, discovery })
}
