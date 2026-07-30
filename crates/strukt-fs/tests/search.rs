use std::fs;
use std::path::Path;

use strukt_fs::{DiscoveryOptions, SearchOptions, discover, quick_open_candidates, search_content};
use tempfile::tempdir;

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
    assert!(!result.truncated);
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
}
