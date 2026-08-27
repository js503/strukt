use crate::SurfaceId;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Activity {
    Files,
    Search,
    SourceControl,
    Sessions,
    Tasks,
    Connections,
    Extensions,
    Settings,
}

impl Activity {
    #[must_use]
    pub fn canvas_surface(self) -> SurfaceId {
        SurfaceId::trusted(match self {
            Self::Files => "files",
            Self::Search => "search",
            Self::SourceControl => "source-control",
            Self::Sessions => "sessions",
            Self::Tasks => "tasks",
            Self::Connections => "connections",
            Self::Extensions => "extensions",
            Self::Settings => "settings",
        })
    }

    #[must_use]
    pub fn sidebar_surface(self) -> Option<SurfaceId> {
        Some(SurfaceId::trusted(match self {
            Self::Files => "files.sidebar",
            Self::Search => "search.sidebar",
            Self::SourceControl => "source-control.sidebar",
            Self::Sessions => "sessions.sidebar",
            Self::Tasks => "tasks.sidebar",
            Self::Connections => "connections.sidebar",
            Self::Extensions => "extensions.sidebar",
            Self::Settings => "settings.sidebar",
        }))
    }
}
