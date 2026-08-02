use strukt_language::{
    CompletionInsertion, CompletionItem, DefinitionAccess, DefinitionTarget, Diagnostic,
    DiagnosticSeverity, DocumentUri, LanguageRange, LspPosition, MarkupContent,
    normalize_completion_items, sanitize_hover_markdown,
};

#[test]
fn normalized_features_are_bounded_and_keep_only_owned_values() {
    let uri = DocumentUri::parse("file:///workspace/src/main.rs").unwrap();
    let diagnostic = Diagnostic::new(
        uri,
        LanguageRange::new(LspPosition::new(0, 0), LspPosition::new(0, 1)).unwrap(),
        DiagnosticSeverity::Error,
        "broken",
    )
    .unwrap();
    assert_eq!(diagnostic.message(), "broken");

    let items = (0..250)
        .map(|index| CompletionItem::plain(format!("item-{index}"), "value"))
        .collect();
    assert_eq!(normalize_completion_items(items).len(), 200);
}

#[test]
fn hover_markdown_removes_embedded_html_images_and_remote_links() {
    let hover = sanitize_hover_markdown(
        "hello <script>alert(1)</script> ![x](https://example.com/x.png) [remote](https://example.com)",
    );

    assert_eq!(hover.kind(), MarkupContent::MARKDOWN);
    assert!(!hover.value().contains("script"));
    assert!(!hover.value().contains("https://"));
}

#[test]
fn hover_markdown_is_capped_on_a_utf8_boundary() {
    let hover = sanitize_hover_markdown(&"😀".repeat(100_000));
    assert!(hover.value().len() <= 256 * 1024);
    assert!(hover.value().is_char_boundary(hover.value().len()));
}

#[test]
fn normalized_completion_and_definition_models_preserve_safe_intent() {
    let item = CompletionItem::new(
        "display",
        Some("detail"),
        CompletionInsertion::Plain("inserted".to_owned()),
        None,
    )
    .unwrap();
    assert_eq!(item.label(), "display");

    let workspace = DefinitionTarget::new(
        DocumentUri::parse("file:///workspace/src/lib.rs").unwrap(),
        LanguageRange::new(LspPosition::new(1, 0), LspPosition::new(1, 2)).unwrap(),
        DefinitionAccess::Workspace,
    );
    let external = DefinitionTarget::new(
        DocumentUri::parse("file:///tmp/outside.rs").unwrap(),
        LanguageRange::new(LspPosition::new(0, 0), LspPosition::new(0, 0)).unwrap(),
        DefinitionAccess::ExternalFile,
    );
    assert!(workspace.can_open_without_confirmation());
    assert!(!external.can_open_without_confirmation());
}
