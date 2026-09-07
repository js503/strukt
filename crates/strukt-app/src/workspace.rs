use std::path::PathBuf;

use strukt_fs::{DiscoveryOptions, DiscoveryReport, discover_report_for_root};
use strukt_persistence::WorkspaceStore;
use strukt_workspace::{WorkspaceRoot, WorkspaceState};

#[derive(Clone, Debug)]
pub struct OpenedWorkspace {
    pub state: WorkspaceState,
    pub discovery: DiscoveryReport,
    pub repository_branch: Option<String>,
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
    let discovery = discover_report_for_root(
        &state.root,
        DiscoveryOptions {
            show_hidden: state.explorer.show_hidden,
            show_ignored: state.explorer.show_ignored,
            ..DiscoveryOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;

    let repository_branch = strukt_remote::RemoteGitSummary::read(state.root.path())
        .ok()
        .and_then(|summary| {
            summary
                .branch
                .or_else(|| summary.detached.then(|| "detached".to_owned()))
        });

    Ok(OpenedWorkspace {
        state,
        discovery,
        repository_branch,
    })
}

#[cfg(test)]
mod tests {
    use super::open_workspace_without_store;

    #[test]
    fn local_git_workspace_reports_its_branch() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");

        let opened = open_workspace_without_store(root).expect("open repository workspace");

        assert!(
            opened
                .repository_branch
                .as_deref()
                .is_some_and(|branch| !branch.is_empty())
        );
    }
}
