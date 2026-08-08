use std::fs;
use std::process::Command;

use strukt_remote::{GitError, RemoteGitSummary};
use tempfile::tempdir;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn summarizes_branch_staged_modified_and_untracked_without_writes() {
    let root = tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    git(root.path(), &["config", "user.name", "Fixture"]);
    fs::write(root.path().join("tracked.txt"), "initial").unwrap();
    git(root.path(), &["add", "tracked.txt"]);
    git(root.path(), &["commit", "-qm", "initial"]);
    fs::write(root.path().join("tracked.txt"), "modified").unwrap();
    fs::write(root.path().join("staged.txt"), "staged").unwrap();
    git(root.path(), &["add", "staged.txt"]);
    fs::write(root.path().join("odd\nname.txt"), "untracked").unwrap();

    let summary = RemoteGitSummary::read(root.path()).unwrap();
    assert!(summary.branch.is_some());
    assert_eq!(summary.modified, 1);
    assert_eq!(summary.staged, 1);
    assert_eq!(summary.untracked, 1);
    assert!(!summary.detached);
}

#[test]
fn reports_non_repository_and_missing_git_cleanly() {
    let root = tempdir().unwrap();
    assert!(matches!(
        RemoteGitSummary::read(root.path()),
        Err(GitError::NotRepository)
    ));
    assert!(matches!(
        RemoteGitSummary::read_with_executable(root.path(), root.path().join("missing-git")),
        Err(GitError::Unavailable(_))
    ));
}
