use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RemoteGitSummary {
    pub branch: Option<String>,
    pub detached: bool,
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
}

impl RemoteGitSummary {
    /// Reads a bounded, side-effect-minimized Git worktree summary.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, non-repository, output, or parse error.
    pub fn read(root: impl AsRef<Path>) -> Result<Self, GitError> {
        Self::read_with_executable(root, PathBuf::from("git"))
    }

    /// Reads through an injected Git executable for deterministic failure tests.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::read`].
    pub fn read_with_executable(
        root: impl AsRef<Path>,
        executable: PathBuf,
    ) -> Result<Self, GitError> {
        let output = Command::new(executable)
            .arg("-C")
            .arg(root.as_ref())
            .args([
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=normal",
            ])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .output()
            .map_err(GitError::Unavailable)?;
        if !output.status.success() {
            return Err(GitError::NotRepository);
        }
        if output.stdout.len() > MAX_GIT_OUTPUT_BYTES || output.stderr.len() > MAX_GIT_OUTPUT_BYTES
        {
            return Err(GitError::OutputTooLarge);
        }
        parse_status(&output.stdout)
    }
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("Git is unavailable: {0}")]
    Unavailable(std::io::Error),
    #[error("remote workspace is not a Git repository")]
    NotRepository,
    #[error("Git status output exceeded its bound")]
    OutputTooLarge,
    #[error("Git status output is invalid")]
    InvalidOutput,
}

fn parse_status(output: &[u8]) -> Result<RemoteGitSummary, GitError> {
    let mut summary = RemoteGitSummary::default();
    let mut records = output.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(branch) = record.strip_prefix(b"# branch.head ") {
            let branch = std::str::from_utf8(branch).map_err(|_| GitError::InvalidOutput)?;
            if branch == "(detached)" {
                summary.detached = true;
            } else {
                summary.branch = Some(branch.to_owned());
            }
            continue;
        }
        match record.first() {
            Some(b'?') => summary.untracked = summary.untracked.saturating_add(1),
            Some(b'1' | b'2' | b'u') => {
                let status = record.get(2..4).ok_or(GitError::InvalidOutput)?;
                if status[0] != b'.' {
                    summary.staged = summary.staged.saturating_add(1);
                }
                if status[1] != b'.' {
                    summary.modified = summary.modified.saturating_add(1);
                }
                if record.first() == Some(&b'2') {
                    let _ = records.next();
                }
            }
            Some(b'#' | b'!') => {}
            _ => return Err(GitError::InvalidOutput),
        }
    }
    Ok(summary)
}
