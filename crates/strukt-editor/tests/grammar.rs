use std::path::Path;

use strukt_editor::{GrammarRegistry, PLAIN_TEXT_GRAMMAR};

#[test]
fn detects_extensions_and_exact_file_names() {
    assert_eq!(
        GrammarRegistry::detect(Path::new("src/main.RS"), None).id,
        "rust"
    );
    assert_eq!(
        GrammarRegistry::detect(Path::new("package.json"), None).id,
        "json"
    );
    assert_eq!(
        GrammarRegistry::detect(Path::new("Cargo.toml"), None).id,
        "toml"
    );
    assert_eq!(
        GrammarRegistry::detect(Path::new("README"), None).id,
        "markdown"
    );
    assert_eq!(
        GrammarRegistry::detect(Path::new(".zshrc"), None).id,
        "shell"
    );
}

#[test]
fn explicit_override_wins_and_unknown_values_fall_back_to_plain_text() {
    assert_eq!(
        GrammarRegistry::detect(Path::new("ambiguous.txt"), Some("python")).id,
        "python"
    );
    assert_eq!(
        GrammarRegistry::detect(Path::new("main.rs"), Some("unknown")),
        &PLAIN_TEXT_GRAMMAR
    );
    assert_eq!(
        GrammarRegistry::detect(Path::new("no-extension"), None),
        &PLAIN_TEXT_GRAMMAR
    );
}

#[test]
fn registry_contains_the_public_alpha_language_set() {
    let ids = GrammarRegistry::all()
        .iter()
        .map(|grammar| grammar.id)
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        vec![
            "rust",
            "javascript",
            "typescript",
            "python",
            "json",
            "toml",
            "markdown",
            "shell",
            "yaml",
            "html",
            "css",
            "plain-text",
        ]
    );
    assert!(
        GrammarRegistry::all()
            .iter()
            .all(|grammar| !grammar.iced_token.is_empty())
    );
}
