use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::{Rgb, ThemeMode, ThemeTokens, ThemeValidationError};

pub const THEME_SCHEMA_VERSION: u16 = 1;
pub const QUIET_PRECISION_THEME_ID: &str = "quiet-precision";

static QUIET_PRECISION_DEFINITION: LazyLock<ThemeDefinitionV1> =
    LazyLock::new(build_quiet_precision_definition);
static QUIET_PRECISION_RESOLVED_TOKENS: LazyLock<[ThemeTokens; 2]> = LazyLock::new(|| {
    [
        QUIET_PRECISION_DEFINITION
            .resolve(ThemeMode::Light)
            .expect("the built-in light theme must remain valid")
            .tokens,
        QUIET_PRECISION_DEFINITION
            .resolve(ThemeMode::Dark)
            .expect("the built-in dark theme must remain valid")
            .tokens,
    ]
});

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThemeId(String);

impl ThemeId {
    /// Creates a stable lowercase theme identifier.
    ///
    /// # Errors
    ///
    /// Returns an error when the identifier is empty, too long, begins or ends
    /// with a separator, or contains characters other than lowercase ASCII,
    /// decimal digits, and `-`.
    pub fn new(value: impl Into<String>) -> Result<Self, ThemeValidationError> {
        let value = value.into();
        if !is_valid_identifier(&value) {
            return Err(ThemeValidationError::InvalidThemeId(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn quiet_precision() -> Self {
        Self(QUIET_PRECISION_THEME_ID.to_owned())
    }
}

fn is_valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeRole {
    Canvas,
    Panel,
    PanelActive,
    Border,
    TextPrimary,
    TextMuted,
    Accent,
    Focus,
    TerminalBackground,
    TerminalForeground,
    TerminalAnsi0,
    TerminalAnsi1,
    TerminalAnsi2,
    TerminalAnsi3,
    TerminalAnsi4,
    TerminalAnsi5,
    TerminalAnsi6,
    TerminalAnsi7,
    TerminalAnsi8,
    TerminalAnsi9,
    TerminalAnsi10,
    TerminalAnsi11,
    TerminalAnsi12,
    TerminalAnsi13,
    TerminalAnsi14,
    TerminalAnsi15,
    TerminalSelection,
    TerminalCursor,
    TerminalLink,
    TerminalExited,
    TerminalBackpressure,
    ConnectionRemote,
    SessionLive,
    SessionStopped,
    SessionStale,
    SessionUnread,
    SessionAttention,
    SessionActive,
    SessionSelected,
    StatusSuccess,
    StatusWarning,
    DiagnosticError,
    DiagnosticWarning,
    DiagnosticInformation,
    DiagnosticHint,
    EditorBackground,
    EditorForeground,
    EditorGutter,
    EditorActiveLine,
    EditorSelection,
    EditorMatchingBracket,
    EditorDirty,
    EditorConflict,
    EditorMissing,
    SyntaxKeyword,
    SyntaxString,
    SyntaxComment,
    SyntaxNumber,
    SyntaxType,
    SyntaxFunction,
    SyntaxPunctuation,
}

impl ThemeRole {
    pub const ALL: [Self; 61] = [
        Self::Canvas,
        Self::Panel,
        Self::PanelActive,
        Self::Border,
        Self::TextPrimary,
        Self::TextMuted,
        Self::Accent,
        Self::Focus,
        Self::TerminalBackground,
        Self::TerminalForeground,
        Self::TerminalAnsi0,
        Self::TerminalAnsi1,
        Self::TerminalAnsi2,
        Self::TerminalAnsi3,
        Self::TerminalAnsi4,
        Self::TerminalAnsi5,
        Self::TerminalAnsi6,
        Self::TerminalAnsi7,
        Self::TerminalAnsi8,
        Self::TerminalAnsi9,
        Self::TerminalAnsi10,
        Self::TerminalAnsi11,
        Self::TerminalAnsi12,
        Self::TerminalAnsi13,
        Self::TerminalAnsi14,
        Self::TerminalAnsi15,
        Self::TerminalSelection,
        Self::TerminalCursor,
        Self::TerminalLink,
        Self::TerminalExited,
        Self::TerminalBackpressure,
        Self::ConnectionRemote,
        Self::SessionLive,
        Self::SessionStopped,
        Self::SessionStale,
        Self::SessionUnread,
        Self::SessionAttention,
        Self::SessionActive,
        Self::SessionSelected,
        Self::StatusSuccess,
        Self::StatusWarning,
        Self::DiagnosticError,
        Self::DiagnosticWarning,
        Self::DiagnosticInformation,
        Self::DiagnosticHint,
        Self::EditorBackground,
        Self::EditorForeground,
        Self::EditorGutter,
        Self::EditorActiveLine,
        Self::EditorSelection,
        Self::EditorMatchingBracket,
        Self::EditorDirty,
        Self::EditorConflict,
        Self::EditorMissing,
        Self::SyntaxKeyword,
        Self::SyntaxString,
        Self::SyntaxComment,
        Self::SyntaxNumber,
        Self::SyntaxType,
        Self::SyntaxFunction,
        Self::SyntaxPunctuation,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Panel => "panel",
            Self::PanelActive => "panel_active",
            Self::Border => "border",
            Self::TextPrimary => "text_primary",
            Self::TextMuted => "text_muted",
            Self::Accent => "accent",
            Self::Focus => "focus",
            Self::TerminalBackground => "terminal_background",
            Self::TerminalForeground => "terminal_foreground",
            Self::TerminalAnsi0 => "terminal_ansi_0",
            Self::TerminalAnsi1 => "terminal_ansi_1",
            Self::TerminalAnsi2 => "terminal_ansi_2",
            Self::TerminalAnsi3 => "terminal_ansi_3",
            Self::TerminalAnsi4 => "terminal_ansi_4",
            Self::TerminalAnsi5 => "terminal_ansi_5",
            Self::TerminalAnsi6 => "terminal_ansi_6",
            Self::TerminalAnsi7 => "terminal_ansi_7",
            Self::TerminalAnsi8 => "terminal_ansi_8",
            Self::TerminalAnsi9 => "terminal_ansi_9",
            Self::TerminalAnsi10 => "terminal_ansi_10",
            Self::TerminalAnsi11 => "terminal_ansi_11",
            Self::TerminalAnsi12 => "terminal_ansi_12",
            Self::TerminalAnsi13 => "terminal_ansi_13",
            Self::TerminalAnsi14 => "terminal_ansi_14",
            Self::TerminalAnsi15 => "terminal_ansi_15",
            Self::TerminalSelection => "terminal_selection",
            Self::TerminalCursor => "terminal_cursor",
            Self::TerminalLink => "terminal_link",
            Self::TerminalExited => "terminal_exited",
            Self::TerminalBackpressure => "terminal_backpressure",
            Self::ConnectionRemote => "connection_remote",
            Self::SessionLive => "session_live",
            Self::SessionStopped => "session_stopped",
            Self::SessionStale => "session_stale",
            Self::SessionUnread => "session_unread",
            Self::SessionAttention => "session_attention",
            Self::SessionActive => "session_active",
            Self::SessionSelected => "session_selected",
            Self::StatusSuccess => "status_success",
            Self::StatusWarning => "status_warning",
            Self::DiagnosticError => "diagnostic_error",
            Self::DiagnosticWarning => "diagnostic_warning",
            Self::DiagnosticInformation => "diagnostic_information",
            Self::DiagnosticHint => "diagnostic_hint",
            Self::EditorBackground => "editor_background",
            Self::EditorForeground => "editor_foreground",
            Self::EditorGutter => "editor_gutter",
            Self::EditorActiveLine => "editor_active_line",
            Self::EditorSelection => "editor_selection",
            Self::EditorMatchingBracket => "editor_matching_bracket",
            Self::EditorDirty => "editor_dirty",
            Self::EditorConflict => "editor_conflict",
            Self::EditorMissing => "editor_missing",
            Self::SyntaxKeyword => "syntax_keyword",
            Self::SyntaxString => "syntax_string",
            Self::SyntaxComment => "syntax_comment",
            Self::SyntaxNumber => "syntax_number",
            Self::SyntaxType => "syntax_type",
            Self::SyntaxFunction => "syntax_function",
            Self::SyntaxPunctuation => "syntax_punctuation",
        }
    }

    #[must_use]
    pub const fn color(self, tokens: &ThemeTokens) -> Rgb {
        match self {
            Self::Canvas => tokens.canvas,
            Self::Panel => tokens.panel,
            Self::PanelActive => tokens.panel_active,
            Self::Border => tokens.border,
            Self::TextPrimary => tokens.text_primary,
            Self::TextMuted => tokens.text_muted,
            Self::Accent => tokens.accent,
            Self::Focus => tokens.focus,
            Self::TerminalBackground => tokens.terminal_background,
            Self::TerminalForeground => tokens.terminal_foreground,
            Self::TerminalAnsi0 => tokens.terminal_ansi[0],
            Self::TerminalAnsi1 => tokens.terminal_ansi[1],
            Self::TerminalAnsi2 => tokens.terminal_ansi[2],
            Self::TerminalAnsi3 => tokens.terminal_ansi[3],
            Self::TerminalAnsi4 => tokens.terminal_ansi[4],
            Self::TerminalAnsi5 => tokens.terminal_ansi[5],
            Self::TerminalAnsi6 => tokens.terminal_ansi[6],
            Self::TerminalAnsi7 => tokens.terminal_ansi[7],
            Self::TerminalAnsi8 => tokens.terminal_ansi[8],
            Self::TerminalAnsi9 => tokens.terminal_ansi[9],
            Self::TerminalAnsi10 => tokens.terminal_ansi[10],
            Self::TerminalAnsi11 => tokens.terminal_ansi[11],
            Self::TerminalAnsi12 => tokens.terminal_ansi[12],
            Self::TerminalAnsi13 => tokens.terminal_ansi[13],
            Self::TerminalAnsi14 => tokens.terminal_ansi[14],
            Self::TerminalAnsi15 => tokens.terminal_ansi[15],
            Self::TerminalSelection => tokens.terminal_selection,
            Self::TerminalCursor => tokens.terminal_cursor,
            Self::TerminalLink => tokens.terminal_link,
            Self::TerminalExited => tokens.terminal_exited,
            Self::TerminalBackpressure => tokens.terminal_backpressure,
            Self::ConnectionRemote => tokens.connection_remote,
            Self::SessionLive => tokens.session_live,
            Self::SessionStopped => tokens.session_stopped,
            Self::SessionStale => tokens.session_stale,
            Self::SessionUnread => tokens.session_unread,
            Self::SessionAttention => tokens.session_attention,
            Self::SessionActive => tokens.session_active,
            Self::SessionSelected => tokens.session_selected,
            Self::StatusSuccess => tokens.status_success,
            Self::StatusWarning => tokens.status_warning,
            Self::DiagnosticError => tokens.diagnostic_error,
            Self::DiagnosticWarning => tokens.diagnostic_warning,
            Self::DiagnosticInformation => tokens.diagnostic_information,
            Self::DiagnosticHint => tokens.diagnostic_hint,
            Self::EditorBackground => tokens.editor_background,
            Self::EditorForeground => tokens.editor_foreground,
            Self::EditorGutter => tokens.editor_gutter,
            Self::EditorActiveLine => tokens.editor_active_line,
            Self::EditorSelection => tokens.editor_selection,
            Self::EditorMatchingBracket => tokens.editor_matching_bracket,
            Self::EditorDirty => tokens.editor_dirty,
            Self::EditorConflict => tokens.editor_conflict,
            Self::EditorMissing => tokens.editor_missing,
            Self::SyntaxKeyword => tokens.syntax_keyword,
            Self::SyntaxString => tokens.syntax_string,
            Self::SyntaxComment => tokens.syntax_comment,
            Self::SyntaxNumber => tokens.syntax_number,
            Self::SyntaxType => tokens.syntax_type,
            Self::SyntaxFunction => tokens.syntax_function,
            Self::SyntaxPunctuation => tokens.syntax_punctuation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeMetricsV1 {
    pub space_1: f32,
    pub space_2: f32,
    pub space_3: f32,
    pub space_4: f32,
    pub radius_small: f32,
    pub radius_medium: f32,
    pub control_height: f32,
    pub row_height: f32,
    pub sidebar_width: f32,
    pub context_width: f32,
    pub drawer_height: f32,
}

impl ThemeMetricsV1 {
    #[must_use]
    pub const fn quiet_precision() -> Self {
        Self {
            space_1: 4.0,
            space_2: 8.0,
            space_3: 12.0,
            space_4: 16.0,
            radius_small: 3.0,
            radius_medium: 6.0,
            control_height: 28.0,
            row_height: 27.0,
            sidebar_width: 218.0,
            context_width: 235.0,
            drawer_height: 205.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeVariantV1 {
    pub palette: BTreeMap<String, Rgb>,
    pub roles: BTreeMap<ThemeRole, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeDefinitionV1 {
    pub schema_version: u16,
    pub id: ThemeId,
    pub display_name: String,
    pub author: String,
    pub variants: BTreeMap<ThemeMode, ThemeVariantV1>,
    pub metrics: ThemeMetricsV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThemeDiagnostic {
    UnknownTheme { requested: ThemeId },
    InvalidTheme { requested: ThemeId, detail: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTheme {
    pub requested_id: ThemeId,
    pub id: ThemeId,
    pub mode: ThemeMode,
    pub tokens: ThemeTokens,
    pub metrics: ThemeMetricsV1,
    pub diagnostic: Option<ThemeDiagnostic>,
}

#[must_use]
pub fn quiet_precision_definition() -> ThemeDefinitionV1 {
    QUIET_PRECISION_DEFINITION.clone()
}

fn build_quiet_precision_definition() -> ThemeDefinitionV1 {
    let variants = [ThemeMode::Light, ThemeMode::Dark]
        .into_iter()
        .map(|mode| {
            let tokens = ThemeTokens::quiet_precision_tokens(mode);
            let mut palette = BTreeMap::new();
            let mut roles = BTreeMap::new();
            for role in ThemeRole::ALL {
                let key = role.as_str().to_owned();
                palette.insert(key.clone(), role.color(&tokens));
                roles.insert(role, key);
            }
            (mode, ThemeVariantV1 { palette, roles })
        })
        .collect();
    ThemeDefinitionV1 {
        schema_version: THEME_SCHEMA_VERSION,
        id: ThemeId::quiet_precision(),
        display_name: "Quiet Precision".to_owned(),
        author: "strukt contributors".to_owned(),
        variants,
        metrics: ThemeMetricsV1::quiet_precision(),
    }
}

pub(crate) fn resolved_quiet_precision_tokens(mode: ThemeMode) -> ThemeTokens {
    match mode {
        ThemeMode::Light => QUIET_PRECISION_RESOLVED_TOKENS[0],
        ThemeMode::Dark => QUIET_PRECISION_RESOLVED_TOKENS[1],
    }
}

pub(crate) fn tokens_from_roles(values: &BTreeMap<ThemeRole, Rgb>) -> ThemeTokens {
    let value = |role| values[&role];
    ThemeTokens {
        canvas: value(ThemeRole::Canvas),
        panel: value(ThemeRole::Panel),
        panel_active: value(ThemeRole::PanelActive),
        border: value(ThemeRole::Border),
        text_primary: value(ThemeRole::TextPrimary),
        text_muted: value(ThemeRole::TextMuted),
        accent: value(ThemeRole::Accent),
        focus: value(ThemeRole::Focus),
        terminal_background: value(ThemeRole::TerminalBackground),
        terminal_foreground: value(ThemeRole::TerminalForeground),
        terminal_ansi: [
            value(ThemeRole::TerminalAnsi0),
            value(ThemeRole::TerminalAnsi1),
            value(ThemeRole::TerminalAnsi2),
            value(ThemeRole::TerminalAnsi3),
            value(ThemeRole::TerminalAnsi4),
            value(ThemeRole::TerminalAnsi5),
            value(ThemeRole::TerminalAnsi6),
            value(ThemeRole::TerminalAnsi7),
            value(ThemeRole::TerminalAnsi8),
            value(ThemeRole::TerminalAnsi9),
            value(ThemeRole::TerminalAnsi10),
            value(ThemeRole::TerminalAnsi11),
            value(ThemeRole::TerminalAnsi12),
            value(ThemeRole::TerminalAnsi13),
            value(ThemeRole::TerminalAnsi14),
            value(ThemeRole::TerminalAnsi15),
        ],
        terminal_selection: value(ThemeRole::TerminalSelection),
        terminal_cursor: value(ThemeRole::TerminalCursor),
        terminal_link: value(ThemeRole::TerminalLink),
        terminal_exited: value(ThemeRole::TerminalExited),
        terminal_backpressure: value(ThemeRole::TerminalBackpressure),
        connection_remote: value(ThemeRole::ConnectionRemote),
        session_live: value(ThemeRole::SessionLive),
        session_stopped: value(ThemeRole::SessionStopped),
        session_stale: value(ThemeRole::SessionStale),
        session_unread: value(ThemeRole::SessionUnread),
        session_attention: value(ThemeRole::SessionAttention),
        session_active: value(ThemeRole::SessionActive),
        session_selected: value(ThemeRole::SessionSelected),
        status_success: value(ThemeRole::StatusSuccess),
        status_warning: value(ThemeRole::StatusWarning),
        diagnostic_error: value(ThemeRole::DiagnosticError),
        diagnostic_warning: value(ThemeRole::DiagnosticWarning),
        diagnostic_information: value(ThemeRole::DiagnosticInformation),
        diagnostic_hint: value(ThemeRole::DiagnosticHint),
        editor_background: value(ThemeRole::EditorBackground),
        editor_foreground: value(ThemeRole::EditorForeground),
        editor_gutter: value(ThemeRole::EditorGutter),
        editor_active_line: value(ThemeRole::EditorActiveLine),
        editor_selection: value(ThemeRole::EditorSelection),
        editor_matching_bracket: value(ThemeRole::EditorMatchingBracket),
        editor_dirty: value(ThemeRole::EditorDirty),
        editor_conflict: value(ThemeRole::EditorConflict),
        editor_missing: value(ThemeRole::EditorMissing),
        syntax_keyword: value(ThemeRole::SyntaxKeyword),
        syntax_string: value(ThemeRole::SyntaxString),
        syntax_comment: value(ThemeRole::SyntaxComment),
        syntax_number: value(ThemeRole::SyntaxNumber),
        syntax_type: value(ThemeRole::SyntaxType),
        syntax_function: value(ThemeRole::SyntaxFunction),
        syntax_punctuation: value(ThemeRole::SyntaxPunctuation),
    }
}
