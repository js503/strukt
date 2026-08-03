//! Provider-independent persistent session domain.

mod catalog;
mod id;

pub use catalog::{
    CatalogError, MAX_PANES_PER_WINDOW, MAX_SESSIONS, MAX_TOTAL_PANES, MAX_WINDOWS_PER_SESSION,
    PaneLifecycle, Session, SessionCatalog, SessionLayoutNode, SessionPane, SessionWindow,
};
pub use id::{IdError, PaneId, SessionId, WindowId};
