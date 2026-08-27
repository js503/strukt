#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Files,
    Search,
    SourceControl,
    Sessions,
    Tasks,
    Connect,
    Extensions,
    Settings,
    Close,
    Promote,
    Demote,
    More,
    Warning,
    Error,
}

impl Icon {
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Files => "▱",
            Self::Search => "⌕",
            Self::SourceControl => "⑂",
            Self::Sessions => "▤",
            Self::Tasks => "✓",
            Self::Connect => "↗",
            Self::Extensions => "◇",
            Self::Settings => "⚙",
            Self::Close => "×",
            Self::Promote => "↥",
            Self::Demote => "↧",
            Self::More => "•••",
            Self::Warning => "△",
            Self::Error => "!",
        }
    }
}
