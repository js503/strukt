use std::path::PathBuf;

use strukt_fs::{DiscoveryOptions, DiscoveryReport, discover_report};
use strukt_persistence::WorkspaceStore;
use strukt_workspace::{WorkspaceRoot, WorkspaceState};

#[derive(Clone, Debug)]
pub struct OpenedWorkspace {
    pub state: WorkspaceState,
    pub discovery: DiscoveryReport,
}

pub fn open_workspace(path: PathBuf) -> Result<OpenedWorkspace, String> {
    let store = WorkspaceStore::platform_default().map_err(|error| error.to_string())?;
    open_workspace_with_store(path, &store)
}

pub(crate) fn open_workspace_with_store(
    path: PathBuf,
    store: &WorkspaceStore,
) -> Result<OpenedWorkspace, String> {
    let root = WorkspaceRoot::open(path).map_err(|error| error.to_string())?;
    let state = store
        .load(root.id())
        .map_err(|error| error.to_string())?
        .map_or_else(
            || WorkspaceState::new(root.clone()),
            |snapshot| snapshot.state,
        );
    let discovery = discover_report(
        root.path(),
        DiscoveryOptions {
            show_hidden: state.explorer.show_hidden,
            show_ignored: state.explorer.show_ignored,
            ..DiscoveryOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;

    Ok(OpenedWorkspace { state, discovery })
}
