use strukt_shell::{
    CommandCatalog, CommandContribution, CommandError, CommandId, ExecutionBoundary,
};

fn command(
    id: &str,
    title: &str,
    category: &str,
    keywords: &[&str],
    boundary: ExecutionBoundary,
    enabled: bool,
) -> CommandContribution {
    CommandContribution {
        id: CommandId(id.to_owned()),
        title: title.to_owned(),
        category: category.to_owned(),
        keywords: keywords
            .iter()
            .map(|keyword| (*keyword).to_owned())
            .collect(),
        shortcut: None,
        boundary,
        enabled,
    }
}

#[test]
fn registration_order_is_stable_and_duplicate_ids_are_rejected() {
    let mut catalog = CommandCatalog::default();
    catalog
        .register(command(
            "workspace.open-folder",
            "Open Folder",
            "Workspace",
            &["project"],
            ExecutionBoundary::Local,
            true,
        ))
        .expect("register first command");
    catalog
        .register(command(
            "view.toggle-context",
            "Toggle Context",
            "View",
            &["panel"],
            ExecutionBoundary::Interface,
            true,
        ))
        .expect("register second command");

    assert_eq!(
        catalog
            .search("")
            .iter()
            .map(|entry| entry.command.id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["workspace.open-folder", "view.toggle-context"]
    );
    assert_eq!(
        catalog.register(command(
            "workspace.open-folder",
            "Duplicate",
            "Workspace",
            &[],
            ExecutionBoundary::Local,
            true,
        )),
        Err(CommandError::DuplicateId(CommandId(
            "workspace.open-folder".to_owned()
        )))
    );
}

#[test]
fn search_matches_case_insensitive_tokens_and_filters_categories() {
    let mut catalog = CommandCatalog::default();
    catalog
        .register(command(
            "terminal.new",
            "New Terminal",
            "Terminal",
            &["shell", "console"],
            ExecutionBoundary::Local,
            true,
        ))
        .expect("register terminal");
    catalog
        .register(command(
            "session.new",
            "New Persistent Session",
            "Sessions",
            &["terminal", "tmux"],
            ExecutionBoundary::Remote,
            true,
        ))
        .expect("register session");

    assert_eq!(catalog.search("NEW term").len(), 2);
    assert_eq!(
        catalog.search_in_category("new", Some("sessions"))[0]
            .command
            .id
            .0,
        "session.new"
    );
}

#[test]
fn disabled_commands_remain_discoverable_and_selection_is_only_an_id() {
    let mut catalog = CommandCatalog::default();
    catalog
        .register(command(
            "remote.connect",
            "Connect to Remote",
            "Remote",
            &["ssh"],
            ExecutionBoundary::Remote,
            false,
        ))
        .expect("register remote command");

    let result = catalog.search("ssh");
    assert_eq!(result.len(), 1);
    assert!(!result[0].command.enabled);
    assert_eq!(
        catalog.select(0, &result),
        Some(CommandId("remote.connect".to_owned()))
    );
    assert_eq!(catalog.select(1, &result), None);
}

#[test]
fn execution_boundaries_have_explicit_labels() {
    assert_eq!(ExecutionBoundary::Local.label(None), "LOCAL");
    assert_eq!(ExecutionBoundary::Remote.label(Some("dev-ec2")), "dev-ec2");
    assert_eq!(ExecutionBoundary::Remote.label(None), "REMOTE");
    assert_eq!(ExecutionBoundary::Interface.label(None), "INTERFACE");
}
