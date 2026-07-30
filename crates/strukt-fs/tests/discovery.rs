use std::fs;
use std::path::Path;

use strukt_fs::{
    DiscoveryError, DiscoveryOptions, discover, discover_report, discover_report_for_root,
};
use strukt_workspace::WorkspaceRoot;
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
fn explicit_visibility_reveals_files_ignored_by_dot_ignore() {
    let root = tempdir().unwrap();
    fs::write(root.path().join(".ignore"), "generated.txt\n").unwrap();
    fs::write(root.path().join("generated.txt"), "generated").unwrap();

    let entries = discover(
        root.path(),
        DiscoveryOptions {
            show_hidden: true,
            show_ignored: true,
            ..DiscoveryOptions::default()
        },
    )
    .unwrap();

    let generated = entries
        .iter()
        .find(|entry| entry.relative_path == Path::new("generated.txt"))
        .expect(".ignore-hidden file should be revealed");
    assert!(generated.ignored);
}

#[test]
fn capability_discovery_preserves_nested_ignore_visibility() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::write(root.path().join("nested/.gitignore"), "generated.txt\n").unwrap();
    fs::write(root.path().join("nested/generated.txt"), "generated").unwrap();
    fs::write(root.path().join("nested/source.txt"), "source").unwrap();
    let workspace = WorkspaceRoot::open(root.path()).unwrap();

    let hidden = discover_report_for_root(&workspace, DiscoveryOptions::default()).unwrap();
    assert!(
        hidden
            .entries
            .iter()
            .all(|entry| entry.relative_path != Path::new("nested/generated.txt"))
    );

    let visible = discover_report_for_root(
        &workspace,
        DiscoveryOptions {
            show_hidden: true,
            show_ignored: true,
            ..DiscoveryOptions::default()
        },
    )
    .unwrap();
    assert!(
        visible
            .entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("nested/generated.txt"))
            .expect("ignored file should be visible")
            .ignored
    );
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

#[test]
fn entry_limits_retain_the_lexicographically_first_entry() {
    for _ in 0..10 {
        let root = tempdir().unwrap();
        fs::write(root.path().join("z-last.txt"), "last").unwrap();
        fs::write(root.path().join("a-first.txt"), "first").unwrap();

        let report = discover_report(
            root.path(),
            DiscoveryOptions {
                max_entries: 1,
                ..DiscoveryOptions::default()
            },
        )
        .unwrap();

        assert_eq!(report.entries[0].relative_path, Path::new("a-first.txt"));
        assert!(report.truncated);
    }
}

#[test]
fn regular_files_are_not_discovery_roots() {
    let root = tempdir().unwrap();
    let file = root.path().join("main.rs");
    fs::write(&file, "fn main() {}").unwrap();

    assert!(matches!(
        discover_report(file, DiscoveryOptions::default()),
        Err(DiscoveryError::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput
    ));
}

#[cfg(windows)]
#[test]
fn windows_hidden_attributes_mark_entries_and_descendants_hidden() {
    use std::process::Command;

    let root = tempdir().unwrap();
    let hidden_directory = root.path().join("generated");
    fs::create_dir(&hidden_directory).unwrap();
    fs::write(hidden_directory.join("output.txt"), "generated").unwrap();
    let status = Command::new("attrib")
        .arg("+H")
        .arg(&hidden_directory)
        .status()
        .expect("run attrib");
    assert!(status.success());

    let entries = discover(
        root.path(),
        DiscoveryOptions {
            show_hidden: true,
            ..DiscoveryOptions::default()
        },
    )
    .unwrap();

    assert!(
        entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("generated"))
            .expect("hidden directory should be revealed")
            .hidden
    );
    assert!(
        entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("generated/output.txt"))
            .expect("descendant of hidden directory should be revealed")
            .hidden
    );
}
