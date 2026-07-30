use std::fs;
use std::path::Path;

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
    let paths: Vec<_> = entries
        .iter()
        .map(|entry| entry.relative_path.as_path())
        .collect();

    assert!(paths.contains(&Path::new("main.rs")));
    assert!(!paths.contains(&Path::new(".env")));
    assert!(!paths.contains(&Path::new("target/generated.rs")));
    assert!(!paths.contains(&Path::new("node_modules/dependency.js")));
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

    assert!(
        entries
            .iter()
            .any(|entry| entry.relative_path == Path::new(".env"))
    );
    let generated = entries
        .iter()
        .find(|entry| entry.relative_path == Path::new("target/generated.rs"))
        .expect("ignored file should be revealed");
    assert!(generated.ignored);
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
