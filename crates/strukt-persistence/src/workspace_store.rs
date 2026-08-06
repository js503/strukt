use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use strukt_workspace::{WorkspaceId, WorkspaceState};
use thiserror::Error;

use crate::language_store::contribution_is_valid as language_contribution_is_valid;
use crate::session_store::contribution_is_valid as session_contribution_is_valid;
use crate::terminal_store::contribution_is_valid as terminal_contribution_is_valid;

const CURRENT_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceSnapshot {
    pub schema_version: u32,
    pub state: WorkspaceState,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecentWorkspaces {
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceStore {
    root: PathBuf,
}

impl WorkspaceStore {
    /// Creates a store under the platform-specific local application-data directory.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::ApplicationDataUnavailable`] when the platform does not
    /// provide an application-data directory.
    pub fn platform_default() -> Result<Self, StoreError> {
        let dirs = ProjectDirs::from("dev", "strukt", "strukt")
            .ok_or(StoreError::ApplicationDataUnavailable)?;
        Ok(Self::at(dirs.data_local_dir().join("workspaces")))
    }

    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn current_path(&self, id: &WorkspaceId) -> PathBuf {
        self.root.join(format!("{}.json", id.as_str()))
    }

    fn backup_path(&self, id: &WorkspaceId) -> PathBuf {
        self.root.join(format!("{}.last-valid.json", id.as_str()))
    }

    /// Saves the workspace state using atomic file replacement.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when the store directory or snapshot files cannot
    /// be written, and [`StoreError::Json`] when the state cannot be serialized.
    pub fn save(&self, state: &WorkspaceState) -> Result<(), StoreError> {
        let id = state.root.id();
        let current = self.current_path(id);
        let snapshot = WorkspaceSnapshot {
            schema_version: CURRENT_SCHEMA,
            state: state.clone(),
        };
        write_recoverable(&current, &self.backup_path(id), &snapshot, |previous| {
            previous.schema_version == CURRENT_SCHEMA
                && previous.state.root.id() == id
                && contributions_are_valid(&previous.state)
        })
    }

    /// Loads the first valid snapshot from the current or last-valid file.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when a candidate cannot be read for a reason other
    /// than not existing. Invalid persisted JSON is treated as recoverable.
    pub fn load(&self, id: &WorkspaceId) -> Result<Option<WorkspaceSnapshot>, StoreError> {
        for path in [self.current_path(id), self.backup_path(id)] {
            if let Some(snapshot) = Self::read_valid(&path, id)? {
                return Ok(Some(snapshot));
            }
        }
        Ok(None)
    }

    /// Loads a workspace snapshot, including the path-only storage key used by
    /// earlier versions, and rehydrates it with the current retained
    /// capability and volume-aware identity.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when a candidate cannot be read for a reason
    /// other than not existing.
    pub fn load_for_root(
        &self,
        root: &strukt_workspace::WorkspaceRoot,
    ) -> Result<Option<WorkspaceSnapshot>, StoreError> {
        if let Some(snapshot) = self.load(root.id())? {
            return Ok(Some(snapshot));
        }

        let legacy_id = root.legacy_path_id();
        for path in [self.current_path(legacy_id), self.backup_path(legacy_id)] {
            if let Some(snapshot) = Self::read_valid(&path, root.id())? {
                return Ok(Some(snapshot));
            }
        }
        Ok(None)
    }

    /// Loads recent workspace paths, falling back to the last valid record.
    ///
    /// Invalid JSON is recoverable and produces the backup or an empty list.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] for non-missing filesystem errors.
    pub fn load_recent(&self) -> Result<RecentWorkspaces, StoreError> {
        for path in [
            self.root.join("recent.json"),
            self.root.join("recent.last-valid.json"),
        ] {
            match read_json::<RecentWorkspaces>(&path) {
                Ok(recent) => return Ok(recent),
                Err(StoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(StoreError::Json(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(RecentWorkspaces::default())
    }

    /// Records `root` as the most recently opened workspace.
    ///
    /// # Errors
    ///
    /// Returns a store error when the record cannot be loaded or replaced.
    pub fn record_recent(&self, root: &strukt_workspace::WorkspaceRoot) -> Result<(), StoreError> {
        let mut recent = self.load_recent()?;
        recent.paths.retain(|path| path != root.path());
        recent.paths.insert(0, root.path().to_path_buf());
        recent.paths.truncate(20);
        self.write_recent(&recent)
    }

    /// Removes `path` from the recent-workspace list.
    ///
    /// # Errors
    ///
    /// Returns a store error when the record cannot be loaded or replaced.
    pub fn remove_recent(&self, path: &Path) -> Result<RecentWorkspaces, StoreError> {
        let mut recent = self.load_recent()?;
        recent.paths.retain(|candidate| candidate != path);
        self.write_recent(&recent)?;
        Ok(recent)
    }

    /// Replaces a stale recent path with a newly located workspace.
    ///
    /// # Errors
    ///
    /// Returns a store error when the record cannot be loaded or replaced.
    pub fn relink_recent(
        &self,
        old_path: &Path,
        new_root: &strukt_workspace::WorkspaceRoot,
    ) -> Result<RecentWorkspaces, StoreError> {
        let mut recent = self.load_recent()?;
        recent
            .paths
            .retain(|candidate| candidate != old_path && candidate != new_root.path());
        recent.paths.insert(0, new_root.path().to_path_buf());
        recent.paths.truncate(20);
        self.write_recent(&recent)?;
        Ok(recent)
    }

    fn write_recent(&self, recent: &RecentWorkspaces) -> Result<(), StoreError> {
        write_recoverable(
            &self.root.join("recent.json"),
            &self.root.join("recent.last-valid.json"),
            recent,
            |_| true,
        )
    }

    fn read_valid(path: &Path, id: &WorkspaceId) -> Result<Option<WorkspaceSnapshot>, StoreError> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice::<WorkspaceSnapshot>(&bytes)
                .ok()
                .filter(|snapshot| {
                    snapshot.schema_version == CURRENT_SCHEMA
                        && snapshot.state.root.id() == id
                        && contributions_are_valid(&snapshot.state)
                })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Io(error)),
        }
    }
}

fn contributions_are_valid(state: &WorkspaceState) -> bool {
    terminal_contribution_is_valid(state)
        && language_contribution_is_valid(state)
        && session_contribution_is_valid(state)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, StoreError> {
    let bytes = fs::read(path).map_err(StoreError::Io)?;
    serde_json::from_slice(&bytes).map_err(StoreError::Json)
}

fn write_recoverable<T: DeserializeOwned + Serialize>(
    current: &Path,
    backup: &Path,
    value: &T,
    is_valid: impl FnOnce(&T) -> bool,
) -> Result<(), StoreError> {
    if let Some(parent) = current.parent() {
        fs::create_dir_all(parent).map_err(StoreError::Io)?;
    }
    if let Ok(previous) = read_json::<T>(current)
        && is_valid(&previous)
    {
        write_serialized(backup, &previous)?;
    }
    write_serialized(current, value)
}

fn write_serialized<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(StoreError::Io)?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(StoreError::Json)?;
    let mut file = AtomicWriteFile::open(path).map_err(StoreError::Io)?;
    file.write_all(&bytes).map_err(StoreError::Io)?;
    file.commit().map_err(StoreError::Io)
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("platform application-data directory is unavailable")]
    ApplicationDataUnavailable,
    #[error("workspace state IO failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("workspace state serialization failed: {0}")]
    Json(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use strukt_workspace::{WorkspaceRoot, WorkspaceState};
    use tempfile::tempdir;

    use super::WorkspaceStore;

    #[test]
    fn recents_are_ordered_deduplicated_and_removable() {
        let data = tempdir().unwrap();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let store = WorkspaceStore::at(data.path());
        let first = WorkspaceRoot::open(first.path()).unwrap();
        let second = WorkspaceRoot::open(second.path()).unwrap();

        store.record_recent(&first).unwrap();
        store.record_recent(&second).unwrap();
        store.record_recent(&first).unwrap();

        assert_eq!(
            store.load_recent().unwrap().paths,
            vec![first.path().to_path_buf(), second.path().to_path_buf()]
        );
        assert_eq!(
            store.remove_recent(first.path()).unwrap().paths,
            vec![second.path().to_path_buf()]
        );
    }

    #[test]
    fn corrupt_current_recent_file_falls_back_to_last_valid() {
        let data = tempdir().unwrap();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let store = WorkspaceStore::at(data.path());
        let first = WorkspaceRoot::open(first.path()).unwrap();
        let second = WorkspaceRoot::open(second.path()).unwrap();
        store.record_recent(&first).unwrap();
        store.record_recent(&second).unwrap();
        fs::write(data.path().join("recent.json"), b"not json").unwrap();

        assert_eq!(
            store.load_recent().unwrap().paths,
            vec![first.path().to_path_buf()]
        );
    }

    #[test]
    fn relinking_a_recent_path_moves_the_replacement_to_the_front() {
        let data = tempdir().unwrap();
        let missing = tempdir().unwrap();
        let existing = tempdir().unwrap();
        let replacement = tempdir().unwrap();
        let store = WorkspaceStore::at(data.path());
        let missing = WorkspaceRoot::open(missing.path()).unwrap();
        let existing = WorkspaceRoot::open(existing.path()).unwrap();
        let replacement = WorkspaceRoot::open(replacement.path()).unwrap();
        store.record_recent(&missing).unwrap();
        store.record_recent(&existing).unwrap();

        let recent = store.relink_recent(missing.path(), &replacement).unwrap();

        assert_eq!(
            recent.paths,
            vec![
                replacement.path().to_path_buf(),
                existing.path().to_path_buf()
            ]
        );
    }

    #[test]
    fn workspace_save_still_recovers_from_a_corrupt_current_snapshot() {
        let data = tempdir().unwrap();
        let project = tempdir().unwrap();
        let store = WorkspaceStore::at(data.path());
        let mut first = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
        store.save(&first).unwrap();
        first.explorer.show_hidden = true;
        store.save(&first).unwrap();
        fs::write(store.current_path(first.root.id()), b"not json").unwrap();

        let restored = store.load(first.root.id()).unwrap().unwrap();

        assert!(!restored.state.explorer.show_hidden);
    }
}
