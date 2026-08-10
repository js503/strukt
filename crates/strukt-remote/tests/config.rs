use std::fs;

use strukt_remote::{ConfigDiscoveryLimits, discover_aliases};
use tempfile::tempdir;

#[test]
fn discovers_only_concrete_host_aliases_in_deterministic_order() {
    let root = tempdir().unwrap();
    let config = root.path().join("config");
    fs::write(
        &config,
        r#"
            # user hosts
            Host ec2-development build-box *.internal !blocked
              User ubuntu
            Host "quoted-host" ec2-development
            Host ?attern [abc] catch-all-*
            Host --bad
        "#,
    )
    .unwrap();

    let report = discover_aliases(&[config], &ConfigDiscoveryLimits::default());
    let aliases = report
        .aliases
        .iter()
        .map(strukt_remote::SshAlias::as_str)
        .collect::<Vec<_>>();
    assert_eq!(aliases, ["build-box", "ec2-development", "quoted-host"]);
    assert!(report.warnings.is_empty());
}

#[test]
fn follows_relative_includes_with_globs_and_breaks_cycles() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("conf.d")).unwrap();
    fs::write(
        root.path().join("config"),
        "Include conf.d/*.conf\nHost primary\n",
    )
    .unwrap();
    fs::write(
        root.path().join("conf.d/20-b.conf"),
        "Host zebra\nInclude ../config\n",
    )
    .unwrap();
    fs::write(root.path().join("conf.d/10-a.conf"), "Host alpha beta\n").unwrap();

    let report = discover_aliases(
        &[root.path().join("config")],
        &ConfigDiscoveryLimits::default(),
    );
    let aliases = report
        .aliases
        .iter()
        .map(strukt_remote::SshAlias::as_str)
        .collect::<Vec<_>>();
    assert_eq!(aliases, ["alpha", "beta", "primary", "zebra"]);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("cycle"))
    );
}

#[test]
fn unreadable_and_malformed_inputs_are_bounded_warnings_not_global_failure() {
    let root = tempdir().unwrap();
    let config = root.path().join("config");
    fs::write(
        &config,
        "Host valid\nInclude missing.conf\nHost \"unterminated\nHost also-valid\n",
    )
    .unwrap();
    let limits = ConfigDiscoveryLimits {
        max_files: 4,
        max_depth: 2,
        max_file_bytes: 64,
        max_warnings: 2,
    };

    let report = discover_aliases(&[config, root.path().join("absent")], &limits);
    assert!(report.aliases.iter().any(|alias| alias.as_str() == "valid"));
    assert!(report.warnings.len() <= 2);
    assert!(report.truncated);
}

#[test]
fn include_file_and_depth_limits_stop_unbounded_traversal() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a"), "Include b\nHost alpha\n").unwrap();
    fs::write(root.path().join("b"), "Include c\nHost beta\n").unwrap();
    fs::write(root.path().join("c"), "Host gamma\n").unwrap();
    let limits = ConfigDiscoveryLimits {
        max_files: 2,
        max_depth: 1,
        max_file_bytes: 1_024,
        max_warnings: 8,
    };

    let report = discover_aliases(&[root.path().join("a")], &limits);
    assert!(report.truncated);
    assert!(report.aliases.iter().any(|alias| alias.as_str() == "alpha"));
    assert!(report.aliases.iter().any(|alias| alias.as_str() == "beta"));
    assert!(!report.aliases.iter().any(|alias| alias.as_str() == "gamma"));
}
