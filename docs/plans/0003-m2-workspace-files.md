# M2 Workspace and Files Implementation Plan

- Status: Complete
- Tracking issue: [#3 — M2: Local workspace and files](https://github.com/js503/strukt/issues/3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the first M2 vertical slice: open a local folder as a durable
workspace, browse and search real files, perform safe file operations, react to
filesystem changes, and restore workspace/explorer state without modifying the
repository.

**Architecture:** New UI-independent Rust crates own workspace identity,
ignore-aware filesystem discovery, watching, file operations, and versioned local
persistence. `strukt-app` uses asynchronous Iced tasks to invoke those capabilities
and renders their immutable view state. Platform behavior remains behind contracts
that later local-terminal and remote-workspace plans can reuse.

**Tech Stack:** Rust 1.97.1, Rust 2024 edition, Iced 0.14, Tokio,
`atomic-write-file 0.3.0`, `ignore 0.4.31`, `notify 8.2.0`, `rfd 0.17.2`,
`directories 6.0.0`, `blake3 1.8.5`, `serde 1.0.229`, `serde_json 1`,
`tempfile 3.27.0`, `trash 5.2.6`, `cap-std 4.0.2`, `cap-fs-ext 4.0.2`,
Cargo tests, GitHub Actions.

---

## Scope boundary

This plan implements delivery slice 1 from
[`docs/specs/0003-local-development-workspace.md`](../specs/0003-local-development-workspace.md).

Included:

- folder-based workspace identity and lifecycle
- native folder selection
- local application-data persistence
- real file explorer with hidden and ignored-file toggles
- ignore-aware Quick Open discovery
- bounded content search
- filesystem watch event normalization and stale/rescan behavior
- create, rename, move, duplicate, trash, and permanent-delete contracts
- recent-workspace restoration
- macOS, Windows, and Linux automated validation

Excluded:

- text-buffer editing and save conflict handling, delivered by the editor plan
- language-server behavior, delivered by the language-intelligence plan
- PTY/ConPTY and terminal rendering, delivered by the local-terminal plan
- multi-root workspaces
- repository-owned `.strukt` configuration
- SSH or remote filesystems

## Delivery-plan sequence

M2 uses five dependent implementation plans:

1. `0003-m2-workspace-files.md` — this plan
2. editor and file-save behavior
3. local terminal and PTY/ConPTY behavior
4. language intelligence
5. integration, restoration, stress validation, and Iced revalidation

Later plans are written only after the preceding contracts are implemented and
reviewed, so their exact file paths and APIs reflect working code rather than
guesses.

## File map

```text
Cargo.toml
Cargo.lock
crates/
├── strukt-workspace/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── identity.rs
│   │   └── state.rs
│   └── tests/
│       ├── workspace_identity.rs
│       └── workspace_state.rs
├── strukt-fs/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── discovery.rs
│   │   ├── operations.rs
│   │   ├── search.rs
│   │   └── watcher.rs
│   └── tests/
│       ├── discovery.rs
│       ├── operations.rs
│       ├── search.rs
│       └── watcher.rs
├── strukt-persistence/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   └── workspace_store.rs
│   └── tests/
│       └── workspace_store.rs
└── strukt-app/
    ├── Cargo.toml
    └── src/
        ├── app.rs
        ├── main.rs
        ├── view.rs
        └── workspace.rs
docs/
├── evidence/
│   └── m2-workspace-files-validation.md
├── plans/
│   └── 0003-m2-workspace-files.md
├── roadmap.md
└── tracker.md
README.md
```

Responsibilities:

- `strukt-workspace`: normalized workspace identity and UI-independent workspace
  state.
- `strukt-fs`: local discovery, ignore behavior, search, watch normalization, and
  file operations.
- `strukt-persistence`: platform application-data location, schema versioning,
  atomic snapshots, and last-valid recovery.
- `strukt-app/src/workspace.rs`: application orchestration and conversion from
  domain results to Iced messages.
- `strukt-app/src/view.rs`: native folder action, explorer, Quick Open, search, and
  visible status only.

### Task 1: Add workspace identity and state crates

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/strukt-workspace/Cargo.toml`
- Create: `crates/strukt-workspace/src/lib.rs`
- Create: `crates/strukt-workspace/src/identity.rs`
- Create: `crates/strukt-workspace/src/state.rs`
- Create: `crates/strukt-workspace/tests/workspace_identity.rs`
- Create: `crates/strukt-workspace/tests/workspace_state.rs`

- [x] **Step 1: Add the crate manifest and failing identity test**

Add `crates/strukt-workspace` to `workspace.members`. Add these workspace
dependencies:

```toml
blake3 = "1.8.5"
atomic-write-file = "0.3.0"
directories = "6.0.0"
ignore = "0.4.31"
notify = "8.2.0"
rfd = "0.17.2"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1"
strukt-workspace = { path = "crates/strukt-workspace" }
tempfile = "3.27.0"
tokio = { version = "1", features = ["rt", "sync"] }
trash = "5.2.6"
```

Create `crates/strukt-workspace/Cargo.toml`:

```toml
[package]
name = "strukt-workspace"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
blake3.workspace = true
serde.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true

[lints]
workspace = true
```

Create `crates/strukt-workspace/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

mod identity;
mod state;

pub use identity::{WorkspaceError, WorkspaceId, WorkspaceRoot};
pub use state::{ExplorerState, WorkspaceState};
```

Create `crates/strukt-workspace/tests/workspace_identity.rs`:

```rust
use strukt_workspace::WorkspaceRoot;
use tempfile::tempdir;

#[test]
fn canonical_paths_produce_stable_workspace_identity() {
    let project = tempdir().expect("temporary project");
    let first = WorkspaceRoot::open(project.path()).expect("open workspace");
    let second = WorkspaceRoot::open(project.path().join(".")).expect("open workspace");

    assert_eq!(first.id(), second.id());
    assert_eq!(first.path(), project.path().canonicalize().unwrap());
}

#[test]
fn regular_files_are_not_workspace_roots() {
    let project = tempdir().expect("temporary project");
    let file = project.path().join("README.md");
    std::fs::write(&file, "strukt").unwrap();

    assert!(WorkspaceRoot::open(file).is_err());
}
```

- [x] **Step 2: Run the identity test and verify RED**

Run:

```bash
cargo test -p strukt-workspace --test workspace_identity
```

Expected: FAIL because `identity.rs`, `state.rs`, and the exported workspace types do
not exist.

- [x] **Step 3: Implement normalized workspace identity**

Create `crates/strukt-workspace/src/identity.rs`:

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceRoot {
    id: WorkspaceId,
    path: PathBuf,
    display_name: String,
}

impl WorkspaceRoot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let requested = path.as_ref();
        let path = requested
            .canonicalize()
            .map_err(|source| WorkspaceError::Access {
                path: requested.to_path_buf(),
                source,
            })?;
        if !path.is_dir() {
            return Err(WorkspaceError::NotDirectory(path));
        }

        let identity_bytes = path.to_string_lossy();
        let id = WorkspaceId(blake3::hash(identity_bytes.as_bytes()).to_hex().to_string());
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map_or_else(|| path.display().to_string(), ToOwned::to_owned);

        Ok(Self {
            id,
            path,
            display_name,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &WorkspaceId {
        &self.id
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("cannot access workspace path {path}: {source}")]
    Access {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("workspace root is not a directory: {0}")]
    NotDirectory(PathBuf),
}
```

Create `crates/strukt-workspace/src/state.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::WorkspaceRoot;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExplorerState {
    pub visible: bool,
    pub show_hidden: bool,
    pub show_ignored: bool,
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            visible: true,
            show_hidden: false,
            show_ignored: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceState {
    pub root: WorkspaceRoot,
    pub explorer: ExplorerState,
    pub stale_filesystem: bool,
}

impl WorkspaceState {
    #[must_use]
    pub fn new(root: WorkspaceRoot) -> Self {
        Self {
            root,
            explorer: ExplorerState::default(),
            stale_filesystem: false,
        }
    }
}
```

- [x] **Step 4: Add and pass workspace-state tests**

Create `crates/strukt-workspace/tests/workspace_state.rs`:

```rust
use strukt_workspace::{WorkspaceRoot, WorkspaceState};
use tempfile::tempdir;

#[test]
fn new_workspace_uses_safe_explorer_defaults() {
    let project = tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());

    assert!(state.explorer.visible);
    assert!(!state.explorer.show_hidden);
    assert!(!state.explorer.show_ignored);
    assert!(!state.stale_filesystem);
}
```

Run:

```bash
cargo test -p strukt-workspace
```

Expected: all workspace tests PASS.

- [x] **Step 5: Commit workspace identity**

```bash
git add Cargo.toml Cargo.lock crates/strukt-workspace
git commit -m "feat: add local workspace identity"
```

### Task 2: Persist versioned workspace snapshots

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/strukt-persistence/Cargo.toml`
- Create: `crates/strukt-persistence/src/lib.rs`
- Create: `crates/strukt-persistence/src/workspace_store.rs`
- Create: `crates/strukt-persistence/tests/workspace_store.rs`

- [x] **Step 1: Add the persistence crate and failing round-trip test**

Add `crates/strukt-persistence` to `workspace.members` and add
`strukt-persistence = { path = "crates/strukt-persistence" }` to
`workspace.dependencies`.

Create `crates/strukt-persistence/Cargo.toml`:

```toml
[package]
name = "strukt-persistence"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
atomic-write-file.workspace = true
directories.workspace = true
serde.workspace = true
serde_json.workspace = true
strukt-workspace.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true

[lints]
workspace = true
```

Create `crates/strukt-persistence/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

mod workspace_store;

pub use workspace_store::{StoreError, WorkspaceSnapshot, WorkspaceStore};
```

Create `crates/strukt-persistence/tests/workspace_store.rs`:

```rust
use strukt_persistence::WorkspaceStore;
use strukt_workspace::{WorkspaceRoot, WorkspaceState};
use tempfile::tempdir;

#[test]
fn snapshots_round_trip_without_touching_the_workspace() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let store = WorkspaceStore::at(app_data.path());

    store.save(&state).unwrap();
    let restored = store.load(state.root.id()).unwrap().unwrap();

    assert_eq!(restored.state, state);
    assert!(!project.path().join(".strukt").exists());
}

#[test]
fn malformed_current_snapshot_falls_back_to_last_valid_snapshot() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let store = WorkspaceStore::at(app_data.path());

    store.save(&state).unwrap();
    store.save(&state).unwrap();
    std::fs::write(store.current_path(state.root.id()), b"{broken").unwrap();

    assert_eq!(store.load(state.root.id()).unwrap().unwrap().state, state);
}
```

- [x] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test -p strukt-persistence --test workspace_store
```

Expected: FAIL because `WorkspaceStore` is not defined.

- [x] **Step 3: Implement schema versioning and last-valid recovery**

Create `crates/strukt-persistence/src/workspace_store.rs`:

```rust
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use strukt_workspace::{WorkspaceId, WorkspaceState};
use thiserror::Error;

const CURRENT_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceSnapshot {
    pub schema_version: u32,
    pub state: WorkspaceState,
}

#[derive(Clone, Debug)]
pub struct WorkspaceStore {
    root: PathBuf,
}

impl WorkspaceStore {
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

    pub fn save(&self, state: &WorkspaceState) -> Result<(), StoreError> {
        fs::create_dir_all(&self.root).map_err(StoreError::Io)?;
        let current = self.current_path(state.root.id());
        let backup = self.backup_path(state.root.id());
        let bytes = serde_json::to_vec_pretty(&WorkspaceSnapshot {
            schema_version: CURRENT_SCHEMA,
            state: state.clone(),
        })
        .map_err(StoreError::Json)?;

        if current.exists() {
            fs::copy(&current, &backup).map_err(StoreError::Io)?;
        }
        let mut file = AtomicWriteFile::open(&current).map_err(StoreError::Io)?;
        file.write_all(&bytes).map_err(StoreError::Io)?;
        file.commit().map_err(StoreError::Io)
    }

    pub fn load(&self, id: &WorkspaceId) -> Result<Option<WorkspaceSnapshot>, StoreError> {
        for path in [self.current_path(id), self.backup_path(id)] {
            match fs::read(&path) {
                Ok(bytes) => {
                    if let Ok(snapshot) = serde_json::from_slice::<WorkspaceSnapshot>(&bytes)
                        && snapshot.schema_version == CURRENT_SCHEMA
                    {
                        return Ok(Some(snapshot));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
        Ok(None)
    }
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
```

The implementation atomically replaces the current snapshot and preserves the
previous parseable snapshot as a fallback. Task 10 must record that Windows
replacement semantics pass in hosted CI before the plan is complete.

- [x] **Step 4: Run persistence tests and verify GREEN**

Run:

```bash
cargo test -p strukt-persistence
```

Expected: both persistence tests PASS.

- [x] **Step 5: Commit persistence**

```bash
git add Cargo.toml Cargo.lock crates/strukt-persistence
git commit -m "feat: persist local workspace state"
```

### Task 3: Discover real files with explicit visibility controls

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/strukt-fs/Cargo.toml`
- Create: `crates/strukt-fs/src/lib.rs`
- Create: `crates/strukt-fs/src/discovery.rs`
- Create: `crates/strukt-fs/tests/discovery.rs`

- [x] **Step 1: Add the filesystem crate and failing discovery tests**

Add `crates/strukt-fs` to `workspace.members` and add
`strukt-fs = { path = "crates/strukt-fs" }` to `workspace.dependencies`.

Create `crates/strukt-fs/Cargo.toml`:

```toml
[package]
name = "strukt-fs"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
ignore.workspace = true
notify.workspace = true
serde.workspace = true
thiserror.workspace = true
trash.workspace = true

[dev-dependencies]
tempfile.workspace = true

[lints]
workspace = true
```

Create `crates/strukt-fs/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

mod discovery;
mod operations;
mod search;
mod watcher;

pub use discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, FileEntry, FileKind, discover,
    discover_report,
};
pub use operations::{FileOperation, OperationError, apply_operation};
pub use search::{SearchMatch, SearchOptions, search_content};
pub use watcher::{FileEvent, WorkspaceWatcher, WatcherError};
```

Create `crates/strukt-fs/tests/discovery.rs`:

```rust
use std::fs;

use strukt_fs::{DiscoveryOptions, discover, discover_report};
use tempfile::tempdir;

#[test]
fn default_discovery_hides_ignored_and_hidden_files() {
    let root = tempdir().unwrap();
    fs::write(root.path().join(".gitignore"), "target/\n").unwrap();
    fs::create_dir(root.path().join("target")).unwrap();
    fs::write(root.path().join("target/generated.rs"), "generated").unwrap();
    fs::create_dir(root.path().join("node_modules")).unwrap();
    fs::write(root.path().join("node_modules/dependency.js"), "generated").unwrap();
    fs::write(root.path().join(".env"), "secret").unwrap();
    fs::write(root.path().join("main.rs"), "fn main() {}").unwrap();

    let entries = discover(root.path(), DiscoveryOptions::default()).unwrap();
    let paths: Vec<_> = entries.iter().map(|entry| entry.relative_path.as_path()).collect();

    assert!(paths.contains(&std::path::Path::new("main.rs")));
    assert!(!paths.contains(&std::path::Path::new(".env")));
    assert!(!paths.contains(&std::path::Path::new("target/generated.rs")));
    assert!(!paths.contains(&std::path::Path::new("node_modules/dependency.js")));
}

#[test]
fn explicit_visibility_reveals_hidden_and_ignored_files() {
    let root = tempdir().unwrap();
    fs::write(root.path().join(".gitignore"), "target/\n").unwrap();
    fs::create_dir(root.path().join("target")).unwrap();
    fs::write(root.path().join("target/generated.rs"), "generated").unwrap();
    fs::write(root.path().join(".env"), "secret").unwrap();

    let entries = discover(
        root.path(),
        DiscoveryOptions {
            show_hidden: true,
            show_ignored: true,
            max_entries: 10_000,
        },
    )
    .unwrap();

    assert!(entries.iter().any(|entry| entry.relative_path == ".env".into()));
    assert!(entries
        .iter()
        .any(|entry| entry.relative_path == "target/generated.rs".into()));
    assert!(entries
        .iter()
        .find(|entry| entry.relative_path == "target/generated.rs".into())
        .unwrap()
        .ignored);
}

#[test]
fn entry_limits_return_a_visible_partial_report() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("one.txt"), "one").unwrap();
    fs::write(root.path().join("two.txt"), "two").unwrap();

    let report = discover_report(
        root.path(),
        DiscoveryOptions {
            max_entries: 1,
            ..DiscoveryOptions::default()
        },
    )
    .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert!(report.truncated);
}
```

- [x] **Step 2: Run discovery tests and verify RED**

Run:

```bash
cargo test -p strukt-fs --test discovery
```

Expected: FAIL because the filesystem modules and discovery types do not exist.

- [x] **Step 3: Implement bounded ignore-aware discovery**

Create `crates/strukt-fs/src/discovery.rs`:

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiscoveryOptions {
    pub show_hidden: bool,
    pub show_ignored: bool,
    pub max_entries: usize,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            show_hidden: false,
            show_ignored: false,
            max_entries: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FileKind {
    Directory,
    File,
    Symlink,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileEntry {
    pub relative_path: PathBuf,
    pub kind: FileKind,
    pub depth: usize,
    pub hidden: bool,
    pub ignored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryReport {
    pub entries: Vec<FileEntry>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

pub fn discover(
    root: impl AsRef<Path>,
    options: DiscoveryOptions,
) -> Result<Vec<FileEntry>, DiscoveryError> {
    Ok(discover_report(root, options)?.entries)
}

pub fn discover_report(
    root: impl AsRef<Path>,
    options: DiscoveryOptions,
) -> Result<DiscoveryReport, DiscoveryError> {
    let root = root.as_ref().canonicalize().map_err(DiscoveryError::Io)?;
    let accepted_paths = if options.show_ignored {
        collect_relative_paths(
            &root,
            DiscoveryOptions {
                show_ignored: false,
                ..options
            },
        )?
    } else {
        HashSet::new()
    };

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut truncated = false;
    for result in walker(&root, options) {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(error.to_string());
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        if entries.len() == options.max_entries {
            truncated = true;
            break;
        }

        let relative_path = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| DiscoveryError::OutsideRoot(entry.path().to_path_buf()))?
            .to_path_buf();
        let file_type = entry
            .file_type()
            .ok_or_else(|| DiscoveryError::MissingType(entry.path().to_path_buf()))?;
        let kind = if file_type.is_dir() {
            FileKind::Directory
        } else if file_type.is_symlink() {
            FileKind::Symlink
        } else {
            FileKind::File
        };
        let hidden = relative_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'));
        entries.push(FileEntry {
            ignored: options.show_ignored && !accepted_paths.contains(&relative_path),
            relative_path,
            kind,
            depth: entry.depth(),
            hidden,
        });
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(DiscoveryReport {
        entries,
        warnings,
        truncated,
    })
}

fn collect_relative_paths(
    root: &Path,
    options: DiscoveryOptions,
) -> Result<HashSet<PathBuf>, DiscoveryError> {
    let mut paths = HashSet::new();
    for result in walker(root, options) {
        let entry = result.map_err(DiscoveryError::Walk)?;
        if entry.depth() > 0 {
            paths.insert(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| DiscoveryError::OutsideRoot(entry.path().to_path_buf()))?
                    .to_path_buf(),
            );
        }
    }
    Ok(paths)
}

fn walker(root: &Path, options: DiscoveryOptions) -> ignore::Walk {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!options.show_hidden)
        .git_ignore(!options.show_ignored)
        .git_global(!options.show_ignored)
        .git_exclude(!options.show_ignored)
        .parents(true);
    let exclude_heavy = !options.show_ignored;
    builder.filter_entry(move |entry| {
        !exclude_heavy
            || !matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "target")
            )
    });
    builder.build()
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("filesystem IO failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("filesystem walk failed: {0}")]
    Walk(#[source] ignore::Error),
    #[error("entry escaped workspace root: {0}")]
    OutsideRoot(PathBuf),
    #[error("entry has no file type: {0}")]
    MissingType(PathBuf),
}
```

Create empty, compiling module files for later tasks:

```rust
// crates/strukt-fs/src/operations.rs
```

```rust
// crates/strukt-fs/src/search.rs
```

```rust
// crates/strukt-fs/src/watcher.rs
```

Temporarily export only discovery types from `lib.rs`; add the remaining exports in
their owning tasks.

- [x] **Step 4: Run discovery tests and verify GREEN**

Run:

```bash
cargo test -p strukt-fs --test discovery
```

Expected: both discovery tests PASS.

- [x] **Step 5: Commit discovery**

```bash
git add Cargo.toml Cargo.lock crates/strukt-fs
git commit -m "feat: discover local workspace files"
```

### Task 4: Add bounded Quick Open and content search

**Files:**

- Modify: `crates/strukt-fs/src/lib.rs`
- Modify: `crates/strukt-fs/src/search.rs`
- Create: `crates/strukt-fs/tests/search.rs`

- [x] **Step 1: Write failing search tests**

Create `crates/strukt-fs/tests/search.rs`:

```rust
use std::fs;

use strukt_fs::{
    DiscoveryOptions, SearchOptions, discover, quick_open_candidates, search_content,
};
use tempfile::tempdir;

#[test]
fn quick_open_candidates_follow_discovery_visibility() {
    let root = tempdir().unwrap();
    fs::write(root.path().join(".gitignore"), "generated.rs\n").unwrap();
    fs::write(root.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.path().join("generated.rs"), "generated").unwrap();

    let entries = discover(root.path(), DiscoveryOptions::default()).unwrap();

    assert!(entries.iter().any(|entry| entry.relative_path == "main.rs".into()));
    assert!(!entries
        .iter()
        .any(|entry| entry.relative_path == "generated.rs".into()));
}

#[test]
fn content_search_is_bounded_and_reports_truncation() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("one.txt"), "needle one\nneedle two\n").unwrap();

    let result = search_content(
        root.path(),
        "needle",
        SearchOptions {
            max_results: 1,
            max_file_bytes: 1024,
            discovery: DiscoveryOptions::default(),
        },
    )
    .unwrap();

    assert_eq!(result.matches.len(), 1);
    assert!(result.truncated);
}

#[test]
fn quick_open_ranks_subsequence_path_matches() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(root.path().join("README.md"), "strukt").unwrap();
    let entries = discover(root.path(), DiscoveryOptions::default()).unwrap();

    let candidates = quick_open_candidates(&entries, "smr", 10);

    assert_eq!(candidates[0].relative_path, "src/main.rs".into());
}
```

- [x] **Step 2: Run search tests and verify RED**

Run:

```bash
cargo test -p strukt-fs --test search
```

Expected: FAIL because search types are not defined.

- [x] **Step 3: Implement bounded UTF-8 content search**

Replace `crates/strukt-fs/src/search.rs` with:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DiscoveryError, DiscoveryOptions, FileEntry, FileKind, discover};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchOptions {
    pub max_results: usize,
    pub max_file_bytes: u64,
    pub discovery: DiscoveryOptions,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 500,
            max_file_bytes: 2 * 1024 * 1024,
            discovery: DiscoveryOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SearchMatch {
    pub relative_path: PathBuf,
    pub line: usize,
    pub preview: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickOpenCandidate {
    pub relative_path: PathBuf,
    pub score: usize,
}

#[must_use]
pub fn quick_open_candidates(
    entries: &[FileEntry],
    query: &str,
    max_results: usize,
) -> Vec<QuickOpenCandidate> {
    let query = query.to_lowercase();
    let mut candidates: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == FileKind::File)
        .filter_map(|entry| {
            let path = entry.relative_path.to_string_lossy().to_lowercase();
            subsequence_score(&path, &query).map(|score| QuickOpenCandidate {
                relative_path: entry.relative_path.clone(),
                score,
            })
        })
        .collect();
    candidates.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    candidates.truncate(max_results);
    candidates
}

fn subsequence_score(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(candidate.len());
    }
    let mut next_index = 0;
    let mut score = 0;
    for query_char in query.chars() {
        let offset = candidate[next_index..].find(query_char)?;
        score += offset;
        next_index += offset + query_char.len_utf8();
    }
    Some(score)
}

pub fn search_content(
    root: impl AsRef<Path>,
    needle: &str,
    options: SearchOptions,
) -> Result<SearchResult, SearchError> {
    let root = root.as_ref();
    let entries = discover(root, options.discovery).map_err(SearchError::Discovery)?;
    let mut matches = Vec::new();

    for entry in entries {
        if entry.kind != FileKind::File {
            continue;
        }
        let path = root.join(&entry.relative_path);
        let metadata = fs::metadata(&path).map_err(SearchError::Io)?;
        if metadata.len() > options.max_file_bytes {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if line.contains(needle) {
                if matches.len() == options.max_results {
                    return Ok(SearchResult {
                        matches,
                        truncated: true,
                    });
                }
                matches.push(SearchMatch {
                    relative_path: entry.relative_path.clone(),
                    line: index + 1,
                    preview: line.trim().to_owned(),
                });
            }
        }
    }
    Ok(SearchResult {
        matches,
        truncated: false,
    })
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("file discovery failed: {0}")]
    Discovery(#[source] DiscoveryError),
    #[error("content search IO failed: {0}")]
    Io(#[source] std::io::Error),
}
```

Export `QuickOpenCandidate`, `SearchError`, `SearchMatch`, `SearchOptions`,
`SearchResult`, `quick_open_candidates`, and `search_content` from `lib.rs`.

- [x] **Step 4: Run search and crate tests**

Run:

```bash
cargo test -p strukt-fs
```

Expected: discovery and search tests PASS.

- [x] **Step 5: Commit search**

```bash
git add crates/strukt-fs
git commit -m "feat: add bounded workspace search"
```

### Task 5: Normalize filesystem watch events

**Files:**

- Modify: `crates/strukt-fs/src/lib.rs`
- Modify: `crates/strukt-fs/src/watcher.rs`
- Create: `crates/strukt-fs/tests/watcher.rs`

- [x] **Step 1: Write failing watcher normalization tests**

Create `crates/strukt-fs/tests/watcher.rs`:

```rust
use std::path::PathBuf;

use notify::{Event, EventKind};
use strukt_fs::{FileEvent, normalize_notify_event};

#[test]
fn notify_paths_are_deduplicated_and_sorted() {
    let event = Event {
        kind: EventKind::Any,
        paths: vec![PathBuf::from("b"), PathBuf::from("a"), PathBuf::from("a")],
        attrs: Default::default(),
    };

    assert_eq!(
        normalize_notify_event(event),
        FileEvent::Changed(vec![PathBuf::from("a"), PathBuf::from("b")])
    );
}

#[test]
fn watcher_errors_mark_the_workspace_stale() {
    assert_eq!(FileEvent::watch_error("overflow"), FileEvent::Stale("overflow".into()));
}
```

- [x] **Step 2: Run watcher tests and verify RED**

Run:

```bash
cargo test -p strukt-fs --test watcher
```

Expected: FAIL because watch types and normalization are absent.

- [x] **Step 3: Implement watcher contract and notify adapter**

Replace `crates/strukt-fs/src/watcher.rs` with:

```rust
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileEvent {
    Changed(Vec<PathBuf>),
    Stale(String),
}

impl FileEvent {
    #[must_use]
    pub fn watch_error(message: impl Into<String>) -> Self {
        Self::Stale(message.into())
    }
}

#[must_use]
pub fn normalize_notify_event(event: Event) -> FileEvent {
    let mut paths = event.paths;
    paths.sort();
    paths.dedup();
    FileEvent::Changed(paths)
}

pub struct WorkspaceWatcher {
    _watcher: RecommendedWatcher,
    events: Receiver<FileEvent>,
}

impl WorkspaceWatcher {
    pub fn start(root: impl AsRef<Path>) -> Result<Self, WatcherError> {
        let (sender, events) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |result| {
            let event = match result {
                Ok(event) => normalize_notify_event(event),
                Err(error) => FileEvent::watch_error(error.to_string()),
            };
            let _ = sender.send(event);
        })
        .map_err(WatcherError::Notify)?;
        watcher
            .watch(root.as_ref(), RecursiveMode::Recursive)
            .map_err(WatcherError::Notify)?;
        Ok(Self {
            _watcher: watcher,
            events,
        })
    }

    pub fn try_recv(&self) -> Option<FileEvent> {
        self.events.try_recv().ok()
    }
}

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("filesystem watcher failed: {0}")]
    Notify(#[source] notify::Error),
}
```

Export watcher types and `normalize_notify_event` from `lib.rs`.

- [x] **Step 4: Run watcher tests and verify GREEN**

Run:

```bash
cargo test -p strukt-fs --test watcher
```

Expected: both watcher tests PASS.

- [x] **Step 5: Commit watcher support**

```bash
git add crates/strukt-fs
git commit -m "feat: watch local workspace files"
```

### Task 6: Add safe local file operations

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/strukt-fs/Cargo.toml`
- Modify: `crates/strukt-fs/src/lib.rs`
- Modify: `crates/strukt-fs/src/operations.rs`
- Create: `crates/strukt-fs/tests/operations.rs`

- [x] **Step 1: Write failing operation tests**

Create `crates/strukt-fs/tests/operations.rs`:

```rust
use std::fs;

use strukt_fs::{FileOperation, apply_operation};
use tempfile::tempdir;

#[test]
fn create_rename_and_duplicate_stay_inside_the_workspace() {
    let root = tempdir().unwrap();
    apply_operation(root.path(), FileOperation::CreateFile("notes.txt".into())).unwrap();
    apply_operation(
        root.path(),
        FileOperation::Rename {
            from: "notes.txt".into(),
            to: "renamed.txt".into(),
        },
    )
    .unwrap();
    apply_operation(
        root.path(),
        FileOperation::Duplicate {
            from: "renamed.txt".into(),
            to: "copy.txt".into(),
        },
    )
    .unwrap();

    assert!(root.path().join("renamed.txt").is_file());
    assert!(root.path().join("copy.txt").is_file());
}

#[test]
fn parent_traversal_is_rejected() {
    let root = tempdir().unwrap();

    assert!(apply_operation(
        root.path(),
        FileOperation::CreateFile("../escape.txt".into())
    )
    .is_err());
}

#[test]
fn permanent_delete_requires_an_explicit_operation() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("delete.txt"), "content").unwrap();

    apply_operation(
        root.path(),
        FileOperation::DeletePermanently("delete.txt".into()),
    )
    .unwrap();

    assert!(!root.path().join("delete.txt").exists());
}

#[cfg(unix)]
#[test]
fn duplicate_rejects_symlinks_that_escape_the_workspace() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(outside.path().join("secret.txt"), root.path().join("link.txt")).unwrap();

    assert!(apply_operation(
        root.path(),
        FileOperation::Duplicate {
            from: "link.txt".into(),
            to: "copy.txt".into(),
        },
    )
    .is_err());
}
```

- [x] **Step 2: Run operation tests and verify RED**

Run:

```bash
cargo test -p strukt-fs --test operations
```

Expected: FAIL because operations are not implemented.

- [x] **Step 3: Implement root-scoped operations**

Add `cap-std = "4.0.2"` and `cap-fs-ext = "4.0.2"` to
`workspace.dependencies`, then enable both workspace dependencies in `strukt-fs`.
File operations must resolve and mutate paths relative to an open
`cap_std::fs::Dir` for the workspace root. This prevents symlink or junction
ancestor swaps from redirecting create, copy, rename, and permanent-delete
operations outside the workspace. Use the public `cap-fs-ext` extension traits for
nonblocking/no-follow behavior rather than `cap-std`'s doc-hidden hooks. The
platform Trash API accepts only ambient paths; `MoveToTrash` therefore retains a
documented best-effort validation boundary under adversarial concurrent path
mutation.

Replace `crates/strukt-fs/src/operations.rs` with:

```rust
use std::fs;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileOperation {
    CreateFile(PathBuf),
    CreateDirectory(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Duplicate { from: PathBuf, to: PathBuf },
    MoveToTrash(PathBuf),
    DeletePermanently(PathBuf),
}

pub fn apply_operation(
    root: impl AsRef<Path>,
    operation: FileOperation,
) -> Result<(), OperationError> {
    let root = root.as_ref().canonicalize().map_err(OperationError::Io)?;
    match operation {
        FileOperation::CreateFile(path) => {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(scoped(&root, &path)?)
                .map_err(OperationError::Io)?;
        }
        FileOperation::CreateDirectory(path) => {
            fs::create_dir(scoped(&root, &path)?).map_err(OperationError::Io)?;
        }
        FileOperation::Rename { from, to } | FileOperation::Move { from, to } => {
            fs::rename(scoped(&root, &from)?, scoped(&root, &to)?)
                .map_err(OperationError::Io)?;
        }
        FileOperation::Duplicate { from, to } => {
            let source = scoped(&root, &from)?;
            let destination = scoped(&root, &to)?;
            copy_path(&root, &source, &destination)?;
        }
        FileOperation::MoveToTrash(path) => {
            trash::delete(scoped(&root, &path)?).map_err(OperationError::Trash)?;
        }
        FileOperation::DeletePermanently(path) => {
            let target = scoped(&root, &path)?;
            if target.is_dir() {
                fs::remove_dir_all(target).map_err(OperationError::Io)?;
            } else {
                fs::remove_file(target).map_err(OperationError::Io)?;
            }
        }
    }
    Ok(())
}

fn scoped(root: &Path, relative: &Path) -> Result<PathBuf, OperationError> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
    {
        return Err(OperationError::OutsideRoot(relative.to_path_buf()));
    }
    let target = root.join(relative);
    let parent = target.parent().ok_or_else(|| {
        OperationError::OutsideRoot(relative.to_path_buf())
    })?;
    let resolved_parent = parent.canonicalize().map_err(OperationError::Io)?;
    if !resolved_parent.starts_with(root) {
        return Err(OperationError::OutsideRoot(relative.to_path_buf()));
    }
    Ok(target)
}

fn copy_path(root: &Path, source: &Path, destination: &Path) -> Result<(), OperationError> {
    let metadata = fs::symlink_metadata(source).map_err(OperationError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(OperationError::SymlinkCopy(source.to_path_buf()));
    }
    let resolved = source.canonicalize().map_err(OperationError::Io)?;
    if !resolved.starts_with(root) {
        return Err(OperationError::OutsideRoot(source.to_path_buf()));
    }
    if metadata.is_dir() {
        fs::create_dir(destination).map_err(OperationError::Io)?;
        for child in fs::read_dir(source).map_err(OperationError::Io)? {
            let child = child.map_err(OperationError::Io)?;
            copy_path(root, &child.path(), &destination.join(child.file_name()))?;
        }
    } else {
        fs::copy(source, destination).map_err(OperationError::Io)?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum OperationError {
    #[error("path escapes workspace root: {0}")]
    OutsideRoot(PathBuf),
    #[error("duplicating symbolic links is not supported: {0}")]
    SymlinkCopy(PathBuf),
    #[error("file operation failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("trash operation failed: {0}")]
    Trash(#[source] trash::Error),
}
```

Export operation types from `lib.rs`.

The final implementation must also:

- preserve permissions when duplicating regular files and directories;
- reject special files before copying;
- copy into a unique private same-parent staging entry, publish only after the copy
  succeeds, and report cleanup failures so retry state is never silently lost;
- resolve the destination parent through the capability and reject case-insensitive,
  symlink, and junction aliases that place a directory copy inside its source;
- use Windows directory-link removal semantics for directory symlinks; and
- reject destination conflicts without adding public error variants.

- [x] **Step 4: Run operation and full filesystem tests**

Run:

```bash
cargo test -p strukt-fs
```

Expected: all filesystem tests PASS.

- [x] **Step 5: Commit safe operations**

```bash
git add Cargo.toml Cargo.lock crates/strukt-fs
git commit -m "feat: add scoped workspace file operations"
```

### Task 7: Orchestrate asynchronous workspace opening in the app

**Files:**

- Modify: `crates/strukt-app/Cargo.toml`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Create: `crates/strukt-app/src/workspace.rs`

- [x] **Step 1: Add app dependencies and failing reducer tests**

Add these dependencies to `crates/strukt-app/Cargo.toml`:

```toml
rfd.workspace = true
strukt-fs.workspace = true
strukt-persistence.workspace = true
strukt-workspace.workspace = true
tokio.workspace = true
```

Declare `mod workspace;` in `main.rs`.

Add these tests to the existing `tests` module:

```rust
use tempfile::tempdir;

#[test]
fn opened_workspace_replaces_the_representative_file_view() {
    let project = tempdir().unwrap();
    std::fs::write(project.path().join("README.md"), "strukt").unwrap();
    let opened = crate::workspace::open_workspace(project.path().to_path_buf()).unwrap();
    let mut app = StruktApp::default();

    let _ = app.update(Message::WorkspaceOpened(Ok(opened)));

    assert_eq!(
        app.workspace.as_ref().unwrap().root.path(),
        project.path().canonicalize().unwrap()
    );
    assert!(app.files.iter().any(|entry| entry.relative_path == "README.md".into()));
}

#[test]
fn visibility_messages_refresh_discovery_options() {
    let mut app = StruktApp::default();

    let _ = app.update(Message::ToggleHiddenFiles);
    let _ = app.update(Message::ToggleIgnoredFiles);

    assert!(app.explorer_options.show_hidden);
    assert!(app.explorer_options.show_ignored);
}
```

- [x] **Step 2: Run focused app tests and verify RED**

Run:

```bash
cargo test -p strukt-app opened_workspace_replaces_the_representative_file_view
cargo test -p strukt-app visibility_messages_refresh_discovery_options
```

Expected: FAIL because workspace orchestration, messages, and state do not exist.

- [x] **Step 3: Implement workspace opening service**

Create `crates/strukt-app/src/workspace.rs`:

```rust
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
    let root = WorkspaceRoot::open(path).map_err(|error| error.to_string())?;
    let store = WorkspaceStore::platform_default().map_err(|error| error.to_string())?;
    let state = store
        .load(root.id())
        .map_err(|error| error.to_string())?
        .map_or_else(|| WorkspaceState::new(root.clone()), |snapshot| snapshot.state);
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
```

- [x] **Step 4: Add app state, messages, and background tasks**

In `app.rs`, add:

```rust
use std::path::PathBuf;

use strukt_fs::{DiscoveryOptions, DiscoveryReport, FileEntry, discover_report};
use strukt_workspace::WorkspaceState;

use crate::workspace::{OpenedWorkspace, open_workspace};
```

Add fields to `StruktApp`:

```rust
pub workspace: Option<WorkspaceState>,
pub files: Vec<FileEntry>,
pub file_warnings: Vec<String>,
pub filesystem_truncated: bool,
pub explorer_options: DiscoveryOptions,
pub workspace_error: Option<String>,
```

Initialize them with `None`, empty vectors, `false`,
`DiscoveryOptions::default()`, and `None`.

Add message variants:

```rust
OpenFolder,
FolderPicked(Option<PathBuf>),
WorkspaceOpened(Result<OpenedWorkspace, String>),
ToggleHiddenFiles,
ToggleIgnoredFiles,
FilesRefreshed(Result<DiscoveryReport, String>),
```

Handle them before the existing shell-action match:

```rust
match message {
    Message::OpenFolder => {
        return Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Open a strukt workspace")
                    .pick_folder()
                    .await
                    .map(|handle| handle.path().to_path_buf())
            },
            Message::FolderPicked,
        );
    }
    Message::FolderPicked(Some(path)) => {
        return Task::perform(
            async move {
                tokio::task::spawn_blocking(move || open_workspace(path))
                    .await
                    .map_err(|error| error.to_string())?
            },
            Message::WorkspaceOpened,
        );
    }
    Message::FolderPicked(None) => return Task::none(),
    Message::WorkspaceOpened(Ok(opened)) => {
        self.explorer_options.show_hidden = opened.state.explorer.show_hidden;
        self.explorer_options.show_ignored = opened.state.explorer.show_ignored;
        self.files = opened.discovery.entries;
        self.file_warnings = opened.discovery.warnings;
        self.filesystem_truncated = opened.discovery.truncated;
        self.workspace = Some(opened.state);
        self.workspace_error = None;
        return Task::none();
    }
    Message::WorkspaceOpened(Err(error)) => {
        self.workspace_error = Some(error);
        return Task::none();
    }
    Message::ToggleHiddenFiles => {
        self.explorer_options.show_hidden = !self.explorer_options.show_hidden;
    }
    Message::ToggleIgnoredFiles => {
        self.explorer_options.show_ignored = !self.explorer_options.show_ignored;
    }
    Message::FilesRefreshed(Ok(report)) => {
        self.files = report.entries;
        self.file_warnings = report.warnings;
        self.filesystem_truncated = report.truncated;
        return Task::none();
    }
    Message::FilesRefreshed(Err(error)) => {
        self.workspace_error = Some(error);
        return Task::none();
    }
    _ => {}
}
```

After either visibility toggle, clone the root and options and return:

```rust
if let Some(workspace) = &mut self.workspace {
    workspace.explorer.show_hidden = self.explorer_options.show_hidden;
    workspace.explorer.show_ignored = self.explorer_options.show_ignored;
    let root = workspace.root.path().to_path_buf();
    let options = self.explorer_options;
    return Task::perform(
        async move {
            tokio::task::spawn_blocking(move || discover_report(root, options))
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())
        },
        Message::FilesRefreshed,
    );
}
```

Keep the existing shell, theme, keyboard, and smoke-test paths unchanged.

- [x] **Step 5: Run app and workspace tests**

Run:

```bash
cargo test -p strukt-app
cargo test -p strukt-workspace
cargo test -p strukt-fs
cargo test -p strukt-persistence
```

Expected: all focused tests PASS.

- [x] **Step 6: Commit app orchestration**

```bash
git add crates/strukt-app Cargo.toml Cargo.lock
git commit -m "feat: open local workspaces"
```

### Task 8: Render the real explorer and workspace controls

**Files:**

- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/main.rs`

- [x] **Step 1: Add view-model assertions**

Add to `main.rs` tests:

```rust
#[test]
fn explorer_labels_use_real_relative_paths() {
    use strukt_fs::{FileEntry, FileKind};

    let label = crate::view::file_entry_label(&FileEntry {
        relative_path: "src/main.rs".into(),
        kind: FileKind::File,
        depth: 2,
        hidden: false,
        ignored: false,
    });

    assert_eq!(label, "    main.rs");
}
```

- [x] **Step 2: Run the view test and verify RED**

Run:

```bash
cargo test -p strukt-app explorer_labels_use_real_relative_paths
```

Expected: FAIL because `file_entry_label` does not exist.

- [x] **Step 3: Replace representative explorer content**

In `view.rs`, import `strukt_fs::{FileEntry, FileKind}` and add:

```rust
pub(crate) fn file_entry_label(entry: &FileEntry) -> String {
    let indent = "  ".repeat(entry.depth);
    let name = entry
        .relative_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| entry.relative_path.to_string_lossy().as_ref());
    let marker = match entry.kind {
        FileKind::Directory => "▸ ",
        FileKind::File => "",
        FileKind::Symlink => "↗ ",
    };
    format!("{indent}{marker}{name}")
}
```

Change the header workspace label to:

```rust
let workspace_label = app.workspace.as_ref().map_or_else(
    || "No folder open".to_owned(),
    |workspace| format!(
        "{}  ·  {}",
        workspace.root.display_name(),
        workspace.root.path().display()
    ),
);
```

Add an `Open Folder…` button sending `Message::OpenFolder`.

Replace the hard-coded explorer entries with:

```rust
let file_rows = app.files.iter().fold(column![].spacing(4), |column, entry| {
    let label = text(file_entry_label(entry));
    let label = if entry.ignored {
        label.color(color(tokens.text_muted))
    } else {
        label
    };
    column.push(label)
});

let controls = row![
    button(if app.explorer_options.show_hidden {
        "Hide hidden"
    } else {
        "Show hidden"
    })
    .on_press(Message::ToggleHiddenFiles),
    button(if app.explorer_options.show_ignored {
        "Hide ignored"
    } else {
        "Show ignored"
    })
    .on_press(Message::ToggleIgnoredFiles),
]
.spacing(6);
```

Render `controls` above `scrollable(file_rows)`. When no workspace exists, render
`Open a folder to browse real files.` When `workspace_error` exists, render it in
the explorer without hiding the open-folder action. Render each `file_warnings`
entry as a non-blocking warning and show `File list truncated` when
`filesystem_truncated` is true.

- [x] **Step 4: Add selected-entry and safe file-operation dialogs**

Add this UI state in `app.rs`:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ExplorerDialog {
    #[default]
    None,
    CreateFile(String),
    CreateDirectory(String),
    Rename { from: PathBuf, to: String },
    Duplicate { from: PathBuf, to: String },
    ConfirmTrash(PathBuf),
    ConfirmPermanentDelete(PathBuf),
}
```

Add `selected_entry: Option<PathBuf>` and `explorer_dialog: ExplorerDialog` to
`StruktApp`. Add these messages:

```rust
SelectExplorerEntry(PathBuf),
BeginCreateFile,
BeginCreateDirectory,
BeginRename,
BeginDuplicate,
BeginTrash,
BeginPermanentDelete,
ExplorerDialogInput(String),
CancelExplorerDialog,
SubmitExplorerDialog,
FileOperationCompleted(Result<(), String>),
```

Build the exact domain operation in one reducer helper:

```rust
fn operation_from_dialog(dialog: &ExplorerDialog) -> Option<strukt_fs::FileOperation> {
    match dialog {
        ExplorerDialog::CreateFile(path) if !path.is_empty() => {
            Some(strukt_fs::FileOperation::CreateFile(path.into()))
        }
        ExplorerDialog::CreateDirectory(path) if !path.is_empty() => {
            Some(strukt_fs::FileOperation::CreateDirectory(path.into()))
        }
        ExplorerDialog::Rename { from, to } if !to.is_empty() => {
            Some(strukt_fs::FileOperation::Rename {
                from: from.clone(),
                to: to.into(),
            })
        }
        ExplorerDialog::Duplicate { from, to } if !to.is_empty() => {
            Some(strukt_fs::FileOperation::Duplicate {
                from: from.clone(),
                to: to.into(),
            })
        }
        ExplorerDialog::ConfirmTrash(path) => {
            Some(strukt_fs::FileOperation::MoveToTrash(path.clone()))
        }
        ExplorerDialog::ConfirmPermanentDelete(path) => {
            Some(strukt_fs::FileOperation::DeletePermanently(path.clone()))
        }
        _ => None,
    }
}
```

`SubmitExplorerDialog` must resolve the current workspace root, run
`apply_operation(root, operation)` through `tokio::task::spawn_blocking`, and map
the result to `FileOperationCompleted`. Success closes the dialog and schedules a
discovery refresh; failure keeps the dialog open and sets `workspace_error`.

Render each file entry as a button sending `SelectExplorerEntry`. Add explorer
toolbar buttons for `New File`, `New Folder`, `Rename`, `Duplicate`, and `Trash`.
Disable selection-dependent actions when no entry is selected. Render a single
inline dialog using `text_input` for path-bearing operations and explicit
`Confirm`, `Cancel`, and `Delete Permanently` actions for destructive operations.
The default delete action is `MoveToTrash`; permanent deletion is never triggered
by the ordinary trash action.

Add reducer tests asserting that:

```rust
assert_eq!(
    operation_from_dialog(&ExplorerDialog::Rename {
        from: "old.txt".into(),
        to: "new.txt".into(),
    }),
    Some(strukt_fs::FileOperation::Rename {
        from: "old.txt".into(),
        to: "new.txt".into(),
    })
);
assert_eq!(operation_from_dialog(&ExplorerDialog::CreateFile(String::new())), None);
```

- [x] **Step 5: Run tests and manually validate the explorer**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p strukt-app
```

Expected:

- automated checks PASS;
- `Open Folder…` opens the native folder dialog;
- selecting the repository replaces representative entries with real paths;
- hidden and ignored toggles refresh the file list;
- create, rename, duplicate, trash, and permanent-delete confirmations target the
  selected relative path;
- the UI remains responsive during discovery.

- [x] **Step 6: Commit the explorer UI**

```bash
git add crates/strukt-app
git commit -m "feat: render real workspace files"
```

### Task 9: Connect watcher refresh, search, persistence, and restoration

**Files:**

- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/workspace.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-persistence/src/workspace_store.rs`

- [x] **Step 1: Write failing restoration and stale-state tests**

Add to `main.rs` tests:

```rust
#[test]
fn stale_watcher_events_mark_the_workspace_for_rescan() {
    let project = tempdir().unwrap();
    let opened = crate::workspace::open_workspace(project.path().to_path_buf()).unwrap();
    let mut app = StruktApp::default();
    let _ = app.update(Message::WorkspaceOpened(Ok(opened)));

    let _ = app.update(Message::FileEvent(strukt_fs::FileEvent::Stale("overflow".into())));

    assert!(app.workspace.as_ref().unwrap().stale_filesystem);
}

#[test]
fn recent_workspace_path_is_persisted_after_open() {
    let project = tempdir().unwrap();
    let opened = crate::workspace::open_workspace(project.path().to_path_buf()).unwrap();
    let store = strukt_persistence::WorkspaceStore::at(tempdir().unwrap().path());

    store.record_recent(&opened.state.root).unwrap();

    assert_eq!(
        store.load_recent().unwrap().paths,
        vec![project.path().canonicalize().unwrap()]
    );
}
```

- [x] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test -p strukt-app stale_watcher_events_mark_the_workspace_for_rescan
```

Expected: FAIL because `Message::FileEvent` is absent.

- [x] **Step 3: Add watcher polling and coalesced refresh**

Extend application workspace state with `Option<WorkspaceWatcher>`. Remove
`#[derive(Debug)]` from `StruktApp` because the native watcher backend is intentionally
opaque. Add:

```rust
FileEvent(strukt_fs::FileEvent),
PersistWorkspace,
WorkspacePersisted(Result<(), String>),
SearchChanged(String),
SearchCompleted(Result<strukt_fs::SearchResult, String>),
```

When a workspace opens, start `WorkspaceWatcher::start(opened.state.root.path())`
before moving `opened.state` into application state. If watcher startup fails, keep
the workspace open, set `stale_filesystem = true`, and expose the error. Add a 250ms
subscription only while a watcher exists:

```rust
let watcher_tick = time::every(Duration::from_millis(250)).map(|_| Message::PollWatcher);
```

On `PollWatcher`, drain available events, coalesce changed paths, and schedule one
discovery refresh. On `FileEvent::Stale(reason)`, set `stale_filesystem = true`,
display `reason`, and schedule a full discovery refresh. Clear the stale flag only
after `FilesRefreshed(Ok(_))`.

- [x] **Step 4: Persist after state-changing actions and restore the last workspace**

Add a `recent.json` record to `WorkspaceStore`:

```rust
use serde::de::DeserializeOwned;
use strukt_workspace::WorkspaceRoot;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecentWorkspaces {
    pub paths: Vec<PathBuf>,
}

impl WorkspaceStore {
    pub fn load_recent(&self) -> Result<RecentWorkspaces, StoreError> {
        for path in [
            self.root.join("recent.json"),
            self.root.join("recent.last-valid.json"),
        ] {
            match read_json::<RecentWorkspaces>(&path) {
                Ok(recent) => return Ok(recent),
                Err(StoreError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(StoreError::Json(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(RecentWorkspaces { paths: Vec::new() })
    }

    pub fn record_recent(&self, root: &WorkspaceRoot) -> Result<(), StoreError> {
        let mut recent = self.load_recent()?;
        recent.paths.retain(|path| path != root.path());
        recent.paths.insert(0, root.path().to_path_buf());
        recent.paths.truncate(20);
        write_recoverable(
            &self.root.join("recent.json"),
            &self.root.join("recent.last-valid.json"),
            &recent,
        )
    }

    pub fn remove_recent(&self, path: &Path) -> Result<RecentWorkspaces, StoreError> {
        let mut recent = self.load_recent()?;
        recent.paths.retain(|candidate| candidate != path);
        write_recoverable(
            &self.root.join("recent.json"),
            &self.root.join("recent.last-valid.json"),
            &recent,
        )?;
        Ok(recent)
    }

    pub fn relink_recent(
        &self,
        old_path: &Path,
        new_root: &WorkspaceRoot,
    ) -> Result<RecentWorkspaces, StoreError> {
        let mut recent = self.remove_recent(old_path)?;
        recent.paths.retain(|candidate| candidate != new_root.path());
        recent.paths.insert(0, new_root.path().to_path_buf());
        recent.paths.truncate(20);
        write_recoverable(
            &self.root.join("recent.json"),
            &self.root.join("recent.last-valid.json"),
            &recent,
        )?;
        Ok(recent)
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, StoreError> {
    let bytes = fs::read(path).map_err(StoreError::Io)?;
    serde_json::from_slice(&bytes).map_err(StoreError::Json)
}

fn write_recoverable<T: Serialize>(
    current: &Path,
    backup: &Path,
    value: &T,
) -> Result<(), StoreError> {
    if let Some(parent) = current.parent() {
        fs::create_dir_all(parent).map_err(StoreError::Io)?;
    }
    if current.exists() {
        fs::copy(current, backup).map_err(StoreError::Io)?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(StoreError::Json)?;
    let mut file = AtomicWriteFile::open(current).map_err(StoreError::Io)?;
    file.write_all(&bytes).map_err(StoreError::Io)?;
    file.commit().map_err(StoreError::Io)
}
```

Refactor `save()` to call `write_recoverable(current, backup, &snapshot)` so
workspace snapshots and recents share the same tested replacement behavior.
Export `RecentWorkspaces` from `strukt-persistence/src/lib.rs`.

After workspace open and explorer-visibility changes, clone `WorkspaceState` and
call `WorkspaceStore::save` through `tokio::task::spawn_blocking`. At application
startup, add `RecentWorkspaceLoaded(Result<Option<PathBuf>, String>)` and boot with:

```rust
pub fn boot(launch_mode: LaunchMode) -> (Self, Task<Message>) {
    let app = Self::new(launch_mode);
    let restore = Task::perform(
        async {
            tokio::task::spawn_blocking(|| {
                let store = WorkspaceStore::platform_default()
                    .map_err(|error| error.to_string())?;
                let recent = store.load_recent().map_err(|error| error.to_string())?;
                Ok(recent.paths.into_iter().find(|path| path.is_dir()))
            })
            .await
            .map_err(|error| error.to_string())?
        },
        Message::RecentWorkspaceLoaded,
    );
    (app, restore)
}
```

Handle `RecentWorkspaceLoaded(Ok(Some(path)))` by returning the same background-open
task used by `FolderPicked(Some(path))`. Handle `Ok(None)` with `Task::none()` and
show the welcome view. Handle `Err(error)` by setting `workspace_error` without
blocking manual folder opening.

In `main.rs`, replace the initializer with:

```rust
move || StruktApp::boot(launch_mode)
```

Iced 0.14 accepts `(State, Task<Message>)` as an application boot result.

Keep all loaded recent paths in `StruktApp::recent_workspaces`. Do not auto-open a
path that no longer exists. In the welcome view, render every recent path; missing
paths receive `Locate`, `Retry`, and `Remove` actions. Add:

```rust
RetryRecentWorkspace(PathBuf),
LocateRecentWorkspace(PathBuf),
RecentWorkspaceLocated {
    old_path: PathBuf,
    new_path: Option<PathBuf>,
},
RemoveRecentWorkspace(PathBuf),
RecentWorkspacesUpdated(Result<RecentWorkspaces, String>),
```

`Retry` dispatches the normal folder-open task. `Locate` opens the native folder
picker and then calls `WorkspaceStore::relink_recent`. `Remove` calls
`WorkspaceStore::remove_recent`. Both store operations run in `spawn_blocking` and
refresh `recent_workspaces` from `RecentWorkspacesUpdated`.

- [x] **Step 5: Add Quick Open and bounded workspace search UI**

Add Quick Open application state:

```rust
pub quick_open_visible: bool,
pub quick_open_query: String,
pub quick_open_results: Vec<strukt_fs::QuickOpenCandidate>,
pub quick_open_include_ignored: bool,
```

Initialize the fields with `false`, `String::new()`, `Vec::new()`, and `false`.
Add:

```rust
ToggleQuickOpen,
QuickOpenChanged(String),
QuickOpenSelected(PathBuf),
ToggleQuickOpenIgnored,
QuickOpenFilesLoaded(Result<Vec<FileEntry>, String>),
```

Map platform Command+P to `ToggleQuickOpen`. The reducer is:

```rust
Message::ToggleQuickOpen => {
    self.quick_open_visible = !self.quick_open_visible;
    self.quick_open_query.clear();
    self.quick_open_results =
        strukt_fs::quick_open_candidates(&self.files, "", 50);
    return Task::none();
}
Message::QuickOpenChanged(query) => {
    self.quick_open_results =
        strukt_fs::quick_open_candidates(&self.files, &query, 50);
    self.quick_open_query = query;
    return Task::none();
}
Message::QuickOpenSelected(path) => {
    self.selected_entry = Some(path);
    self.quick_open_visible = false;
    return Task::none();
}
```

`ToggleQuickOpenIgnored` flips `quick_open_include_ignored` and performs a fresh
discovery using the explorer options with only `show_ignored` overridden. Map the
result to `QuickOpenFilesLoaded`; on success, rank those files with the current
query without changing the explorer's visibility settings.

When `quick_open_visible` is true, render a focused center-canvas panel containing
`text_input("Quick Open", &app.quick_open_query)` and result buttons labeled with
relative paths. Add an independent `Include ignored files` toggle. Selecting a
result records the selected real path without requiring it to be visible in the
explorer. The editor plan will replace the selection-only behavior with direct
document opening.

Render a search input when `Activity::Search` is active. Changes send
`Message::SearchChanged`. Debounce for 200ms, then run `search_content` in
`spawn_blocking` with:

```rust
SearchOptions {
    max_results: 500,
    max_file_bytes: 2 * 1024 * 1024,
    discovery: DiscoveryOptions {
        show_ignored: app.search_include_ignored,
        ..app.explorer_options
    },
}
```

Add `search_include_ignored: bool` and `ToggleSearchIgnored`; render an independent
`Use ignore files` control in the Search activity. Toggling it reruns the current
non-empty query and never changes explorer or Quick Open preferences.

Render each result as `relative/path:line  preview`. Show `Results truncated` when
`SearchResult::truncated` is true. An empty query clears results and does not start
a search.

- [x] **Step 6: Run integration checks**

Run:

```bash
cargo test --workspace
cargo run -p strukt-app
```

Manually validate:

1. open a temporary folder;
2. create and rename files outside `strukt`;
3. confirm the explorer refreshes;
4. add a `.gitignore` entry and confirm default search excludes it;
5. enable ignored files and confirm it becomes visible;
6. restart `strukt` and confirm the workspace and explorer toggles restore;
7. confirm no `.strukt` path exists in the opened repository.

Expected: tests PASS and all seven manual checks succeed.

- [x] **Step 7: Commit watch, search, and restoration**

```bash
git add crates/strukt-app crates/strukt-persistence
git commit -m "feat: restore and refresh local workspaces"
```

### Task 10: Add cross-platform gates and completion evidence

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Create: `docs/evidence/m2-workspace-files-validation.md`
- Modify: `docs/plans/0003-m2-workspace-files.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`

- [x] **Step 1: Add a deterministic workspace-files smoke mode**

Extend `LaunchMode` with:

```rust
WorkspaceFilesSmoke { root: PathBuf }
```

Parse only the exact pair `--workspace-files-smoke <path>`. In this mode:

1. open the supplied folder through `open_workspace`;
2. verify discovery contains a workflow-created sentinel file;
3. persist and reload the workspace snapshot;
4. print
   `strukt workspace files smoke: open, discovery, and persistence passed`;
5. request `iced::exit()`.

Add unit tests that reject missing paths and near-match flags.

- [x] **Step 2: Add native CI smoke fixtures**

After the native build step in `.github/workflows/ci.yml`, add a shell-specific step
on macOS and Ubuntu:

```yaml
- name: Smoke local workspace files
  if: runner.os != 'Windows'
  shell: bash
  run: |
    fixture="$(mktemp -d)"
    printf 'strukt\n' > "$fixture/strukt-smoke.txt"
    cargo run -p strukt-app -- --workspace-files-smoke "$fixture" |
      tee workspace-files-smoke.log
    grep -F "strukt workspace files smoke: open, discovery, and persistence passed" \
      workspace-files-smoke.log
```

Add the Windows equivalent:

```yaml
- name: Smoke local workspace files
  if: runner.os == 'Windows'
  shell: pwsh
  run: |
    $fixture = Join-Path $env:RUNNER_TEMP "strukt-workspace-files-smoke"
    New-Item -ItemType Directory -Force -Path $fixture | Out-Null
    Set-Content -Path (Join-Path $fixture "strukt-smoke.txt") -Value "strukt"
    $output = & cargo run -p strukt-app -- --workspace-files-smoke $fixture 2>&1
    $output | Write-Output
    if ($LASTEXITCODE -ne 0) {
      throw "workspace files smoke exited with code $LASTEXITCODE"
    }
    if (($output -join "`n") -notmatch
        "strukt workspace files smoke: open, discovery, and persistence passed") {
      throw "workspace files smoke marker missing"
    }
```

- [x] **Step 3: Run the complete local verification gate**

Run:

```bash
forj check
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p strukt-app
cargo run -p strukt-app -- --workspace-files-smoke "$PWD"
cargo check -p strukt-app --target x86_64-pc-windows-msvc
cargo check -p strukt-app --target x86_64-unknown-linux-gnu
```

Expected:

- every command exits zero;
- all tests pass;
- the smoke command prints the exact success marker;
- cross-target checks pass;
- only already-documented transitive future-compatibility warnings may remain.

- [x] **Step 4: Record evidence and update delivery artifacts**

Create `docs/evidence/m2-workspace-files-validation.md` with:

- final commit SHA;
- local command results and test counts;
- hosted CI run URL and per-platform results;
- manual macOS opening, explorer, ignored-file, watcher, search, restart, and
  repository-cleanliness checks;
- known limitations and the remaining editor, terminal, language, integration, and
  M9 Windows-human gates.

Update:

- `README.md` to describe real local workspace/file behavior;
- this plan status to `Complete` and all completed checkboxes to `[x]`;
- `docs/tracker.md` to mark the M2 workspace/files workstream complete while keeping
  the overall M2 milestone in progress;
- `docs/roadmap.md` only if evidence changes an exit criterion or dependency.

- [x] **Step 5: Run agentic review**

Review the complete diff against
`docs/specs/0003-local-development-workspace.md`, focusing on:

- path traversal and symlink escape;
- destructive-operation target resolution;
- ignored-file visibility versus default indexing;
- watcher overflow and stale-state recovery;
- UI-thread blocking;
- persistence corruption and Windows replacement behavior;
- cross-platform folder-dialog and path handling;
- test gaps and accidental editor/terminal scope drift.

Fix all critical and important findings. Record accepted minor findings and deferred
gates in the pull request.

- [x] **Step 6: Commit completion evidence**

```bash
git add .github/workflows/ci.yml README.md docs/evidence \
  docs/plans/0003-m2-workspace-files.md docs/roadmap.md docs/tracker.md
git commit -m "docs: record workspace files validation"
```

## Final verification

Before claiming this plan complete, freshly run:

```bash
forj check
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p strukt-app
cargo run -p strukt-app -- --workspace-files-smoke "$PWD"
```

Then confirm the hosted macOS, Windows, and Linux jobs pass at the final head.
