use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::sync::mpsc;
#[cfg(unix)]
use std::time::Duration;

use strukt_fs::{
    DiscoveryOptions, SearchError, SearchOptions, discover, quick_open_candidates,
    search_content as search_with_root,
};
use strukt_workspace::WorkspaceRoot;
use tempfile::tempdir;

fn search_content(
    root: impl AsRef<Path>,
    needle: &str,
    options: SearchOptions,
) -> Result<strukt_fs::SearchResult, SearchError> {
    let workspace = WorkspaceRoot::open(root).unwrap();
    search_with_root(&workspace, needle, options)
}

#[test]
fn quick_open_candidates_follow_default_discovery_visibility() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(root.path().join(".gitignore"), "generated.rs\n").unwrap();
    fs::write(root.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.path().join("generated.rs"), "generated").unwrap();

    let entries = discover(root.path(), DiscoveryOptions::default()).unwrap();
    let candidates = quick_open_candidates(&entries, "", 10);
    let paths: Vec<_> = candidates
        .iter()
        .map(|candidate| candidate.relative_path.as_path())
        .collect();

    assert!(paths.contains(&Path::new("main.rs")));
    assert!(!paths.contains(&Path::new("generated.rs")));
}

#[test]
fn content_search_reports_a_match_beyond_the_result_limit() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("one.txt"), "  needle one  \nneedle two\n").unwrap();

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
    assert_eq!(result.matches[0].line, 1);
    assert_eq!(result.matches[0].preview, "needle one");
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

    assert_eq!(candidates[0].relative_path, Path::new("src/main.rs"));
}

#[test]
fn content_search_skips_files_larger_than_the_read_limit() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("oversized.txt"), b"needle after limit").unwrap();
    fs::write(root.path().join("small.txt"), b"needle").unwrap();

    let result = search_content(
        root.path(),
        "needle",
        SearchOptions {
            max_results: 10,
            max_file_bytes: 6,
            discovery: DiscoveryOptions::default(),
        },
    )
    .unwrap();

    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].relative_path, Path::new("small.txt"));
    assert!(result.truncated);
}

#[test]
fn content_search_skips_binary_and_invalid_utf8_without_failing() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("binary.dat"), b"needle\0binary").unwrap();
    fs::write(root.path().join("invalid.dat"), b"needle\xffinvalid").unwrap();
    fs::write(root.path().join("text.txt"), "needle text").unwrap();

    let result = search_content(root.path(), "needle", SearchOptions::default()).unwrap();

    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].relative_path, Path::new("text.txt"));
    assert!(result.truncated);
}

#[test]
fn content_search_propagates_discovery_truncation() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "no match").unwrap();
    fs::write(root.path().join("z.txt"), "needle").unwrap();

    let result = search_content(
        root.path(),
        "needle",
        SearchOptions {
            discovery: DiscoveryOptions {
                max_entries: 1,
                ..DiscoveryOptions::default()
            },
            ..SearchOptions::default()
        },
    )
    .unwrap();

    assert!(result.matches.is_empty());
    assert!(result.truncated);
}

#[test]
fn content_search_caps_long_utf8_previews() {
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("long.txt"),
        format!("needle {}\n", "é".repeat(1_000)),
    )
    .unwrap();

    let result = search_content(root.path(), "needle", SearchOptions::default()).unwrap();
    let preview = &result.matches[0].preview;

    assert!(preview.len() <= 512);
    assert!(preview.ends_with('…'));
}

#[cfg(unix)]
#[test]
fn content_search_skips_a_fifo_without_blocking() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "needle").unwrap();
    let fifo = root.path().join("pipe");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo")
            .success()
    );

    let root_path = root.path().to_path_buf();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = search_content(root_path, "needle", SearchOptions::default());
        sender.send(result).ok();
    });

    let result = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("FIFO search must not block")
        .unwrap();
    assert_eq!(result.matches.len(), 1);
    assert!(result.truncated);
}

#[cfg(unix)]
#[test]
fn content_search_rejects_a_replaced_workspace_root() {
    let parent = tempdir().unwrap();
    let root_path = parent.path().join("workspace");
    let moved_path = parent.path().join("moved");
    fs::create_dir(&root_path).unwrap();
    fs::write(root_path.join("safe.txt"), "safe").unwrap();
    let root = WorkspaceRoot::open(&root_path).unwrap();

    fs::rename(&root_path, &moved_path).unwrap();
    fs::create_dir(&root_path).unwrap();
    fs::write(root_path.join("secret.txt"), "needle").unwrap();

    assert!(search_with_root(&root, "needle", SearchOptions::default()).is_err());
}
