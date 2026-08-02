use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::LspPosition;

const COMPLETION_ITEM_LIMIT: usize = 200;
const HOVER_CONTENT_LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DocumentUri(Url);

impl DocumentUri {
    /// Parses an absolute document URI.
    ///
    /// # Errors
    ///
    /// Returns [`FeatureError::InvalidUri`] when `value` is not a URI.
    pub fn parse(value: &str) -> Result<Self, FeatureError> {
        Url::parse(value)
            .map(Self)
            .map_err(|_| FeatureError::InvalidUri)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

impl LanguageRange {
    /// Creates an ordered range on one or more lines.
    ///
    /// # Errors
    ///
    /// Returns [`FeatureError::InvalidRange`] when `end` precedes `start`.
    pub fn new(start: LspPosition, end: LspPosition) -> Result<Self, FeatureError> {
        if (end.line, end.character) < (start.line, start.character) {
            return Err(FeatureError::InvalidRange);
        }
        Ok(Self { start, end })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    uri: DocumentUri,
    range: LanguageRange,
    severity: DiagnosticSeverity,
    message: String,
}

impl Diagnostic {
    /// Creates an owned diagnostic suitable for application state.
    ///
    /// # Errors
    ///
    /// Returns [`FeatureError::EmptyMessage`] for an empty message.
    pub fn new(
        uri: DocumentUri,
        range: LanguageRange,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Result<Self, FeatureError> {
        let message = message.into();
        if message.trim().is_empty() {
            return Err(FeatureError::EmptyMessage);
        }
        Ok(Self {
            uri,
            range,
            severity,
            message,
        })
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompletionItem {
    label: String,
    detail: Option<String>,
    insertion: CompletionInsertion,
    documentation: Option<MarkupContent>,
}

impl CompletionItem {
    #[must_use]
    pub fn plain(label: impl Into<String>, insert_text: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: None,
            insertion: CompletionInsertion::Plain(insert_text.into()),
            documentation: None,
        }
    }

    /// Creates a normalized completion item.
    ///
    /// # Errors
    ///
    /// Returns [`FeatureError::EmptyCompletionLabel`] for an empty label.
    pub fn new(
        label: impl Into<String>,
        detail: Option<impl Into<String>>,
        insertion: CompletionInsertion,
        documentation: Option<MarkupContent>,
    ) -> Result<Self, FeatureError> {
        let label = label.into();
        if label.trim().is_empty() {
            return Err(FeatureError::EmptyCompletionLabel);
        }
        Ok(Self {
            label,
            detail: detail.map(Into::into),
            insertion,
            documentation,
        })
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompletionInsertion {
    Plain(String),
    TextEdit {
        range: LanguageRange,
        new_text: String,
    },
}

#[must_use]
pub fn normalize_completion_items(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items.into_iter().take(COMPLETION_ITEM_LIMIT).collect()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MarkupContent {
    kind: String,
    value: String,
}

impl MarkupContent {
    pub const MARKDOWN: &'static str = "markdown";

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[must_use]
pub fn sanitize_hover_markdown(markdown: &str) -> MarkupContent {
    let without_html = strip_angle_blocks(markdown);
    let sanitized = strip_markdown_destinations(&without_html);
    let value = truncate_utf8(sanitized, HOVER_CONTENT_LIMIT);
    MarkupContent {
        kind: MarkupContent::MARKDOWN.to_owned(),
        value,
    }
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

fn strip_angle_blocks(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut inside = false;
    for character in input.chars() {
        match character {
            '<' => inside = true,
            '>' if inside => inside = false,
            _ if !inside => output.push(character),
            _ => {}
        }
    }
    output
}

fn strip_markdown_destinations(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        let image = bytes[index] == b'!'
            && bytes.get(index + 1) == Some(&b'[')
            && find_markdown_destination(input, index + 1).is_some();
        let link = bytes[index] == b'[' && find_markdown_destination(input, index).is_some();
        if image || link {
            let bracket = if image { index + 1 } else { index };
            if let Some((label_end, destination_end)) = find_markdown_destination(input, bracket) {
                if !image {
                    output.push_str(&input[bracket + 1..label_end]);
                }
                index = destination_end + 1;
                continue;
            }
        }
        let character = input[index..].chars().next().expect("valid UTF-8 boundary");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn find_markdown_destination(input: &str, bracket: usize) -> Option<(usize, usize)> {
    let remainder = input.get(bracket + 1..)?;
    let label_offset = remainder.find("](")?;
    let label_end = bracket + 1 + label_offset;
    let destination = input.get(label_end + 2..)?;
    let destination_offset = destination.find(')')?;
    Some((label_end, label_end + 2 + destination_offset))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionAccess {
    Workspace,
    CurrentDocument,
    ExternalFile,
    UnsupportedScheme,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DefinitionTarget {
    uri: DocumentUri,
    range: LanguageRange,
    access: DefinitionAccess,
}

impl DefinitionTarget {
    #[must_use]
    pub const fn new(uri: DocumentUri, range: LanguageRange, access: DefinitionAccess) -> Self {
        Self { uri, range, access }
    }

    #[must_use]
    pub const fn can_open_without_confirmation(&self) -> bool {
        matches!(
            self.access,
            DefinitionAccess::Workspace | DefinitionAccess::CurrentDocument
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FeatureError {
    #[error("document URI is invalid")]
    InvalidUri,
    #[error("range end precedes range start")]
    InvalidRange,
    #[error("diagnostic message is empty")]
    EmptyMessage,
    #[error("completion label is empty")]
    EmptyCompletionLabel,
}
