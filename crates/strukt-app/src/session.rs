use std::path::Path;
use std::sync::{Arc, Mutex};

use directories::ProjectDirs;
use strukt_persistence::{SessionMigrationOutcome, SessionMigrationPlan, plan_session_migration};
use strukt_session::{
    ClientConnectCompletion, ClientConnectIntent, ClientConnectJob, ClientError, ClientHealth,
    ClientRequestCompletion, ClientRequestJob, PaneId, PaneScreenSnapshot, ProviderCatalogSnapshot,
    RequestBody, ResponseBody, SessionClient, SessionId, WindowId,
};
use strukt_workspace::WorkspaceState;
use thiserror::Error;

pub(crate) struct SessionSurfaces {
    client: Option<SessionClient>,
    configuration_error: Option<String>,
    selected_session: Option<SessionId>,
    selected_window: Option<WindowId>,
    selected_pane: Option<PaneId>,
    pending: Option<PendingRequest>,
    migration_plan: Option<SessionMigrationPlan>,
    completed_migration: Option<SessionMigrationPlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingRequest {
    Ordinary,
    Snapshot(PaneId),
    Detach,
    Migration,
}

impl Default for SessionSurfaces {
    fn default() -> Self {
        match platform_client() {
            Ok(client) => Self::with_client(client),
            Err(error) => Self {
                client: None,
                configuration_error: Some(error.to_string()),
                selected_session: None,
                selected_window: None,
                selected_pane: None,
                pending: None,
                migration_plan: None,
                completed_migration: None,
            },
        }
    }
}

impl SessionSurfaces {
    fn with_client(client: SessionClient) -> Self {
        Self {
            client: Some(client),
            configuration_error: None,
            selected_session: None,
            selected_window: None,
            selected_pane: None,
            pending: None,
            migration_plan: None,
            completed_migration: None,
        }
    }

    pub(crate) fn health(&self) -> ClientHealth {
        self.client
            .as_ref()
            .map_or(ClientHealth::Failed, SessionClient::health)
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.configuration_error.as_deref()
    }

    pub(crate) fn catalog(&self) -> Option<&ProviderCatalogSnapshot> {
        self.client.as_ref().and_then(SessionClient::catalog)
    }

    pub(crate) const fn selected_session(&self) -> Option<SessionId> {
        self.selected_session
    }

    pub(crate) const fn selected_window(&self) -> Option<WindowId> {
        self.selected_window
    }

    pub(crate) const fn selected_pane(&self) -> Option<PaneId> {
        self.selected_pane
    }

    pub(crate) fn active_snapshot(&self) -> Option<&PaneScreenSnapshot> {
        self.selected_pane
            .and_then(|pane| self.client.as_ref()?.snapshot(pane))
    }

    pub(crate) fn request_in_flight(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn begin_connect(&mut self) -> Result<ClientConnectJob, SessionUiError> {
        let client = self.client.as_mut().ok_or(SessionUiError::Unavailable)?;
        client
            .begin_connect(ClientConnectIntent::ExplicitAttach)
            .map_err(Into::into)
    }

    pub(crate) fn begin_reconnect(&mut self) -> Result<ClientConnectJob, SessionUiError> {
        let client = self.client.as_mut().ok_or(SessionUiError::Unavailable)?;
        client
            .begin_connect(ClientConnectIntent::Reconnect)
            .map_err(Into::into)
    }

    pub(crate) fn finish_connect(
        &mut self,
        completion: &SessionConnectCompletion,
    ) -> Result<(), SessionUiError> {
        let completion = completion.take().ok_or(SessionUiError::Consumed)?;
        let client = self.client.as_mut().ok_or(SessionUiError::Unavailable)?;
        client.finish_connect(completion)?;
        self.sync_selection();
        Ok(())
    }

    pub(crate) fn begin_request(
        &mut self,
        body: RequestBody,
    ) -> Result<ClientRequestJob, SessionUiError> {
        self.begin_tagged_request(body, PendingRequest::Ordinary)
    }

    pub(crate) fn begin_snapshot(&mut self) -> Result<ClientRequestJob, SessionUiError> {
        let pane = self.selected_pane.ok_or(SessionUiError::NoSelection)?;
        self.begin_tagged_request(
            RequestBody::Snapshot { pane },
            PendingRequest::Snapshot(pane),
        )
    }

    pub(crate) fn begin_detach(&mut self) -> Result<ClientRequestJob, SessionUiError> {
        self.begin_tagged_request(RequestBody::Detach, PendingRequest::Detach)
    }

    pub(crate) fn begin_migration(
        &mut self,
        state: &WorkspaceState,
    ) -> Result<Option<ClientRequestJob>, SessionUiError> {
        let existing = self.catalog().map(ProviderCatalogSnapshot::catalog);
        let SessionMigrationOutcome::Planned(plan) = plan_session_migration(state, existing)?
        else {
            return Ok(None);
        };
        let job = self.begin_tagged_request(
            RequestBody::ImportStoppedCatalog {
                catalog: plan.catalog.clone(),
            },
            PendingRequest::Migration,
        )?;
        self.migration_plan = Some(plan);
        Ok(Some(job))
    }

    pub(crate) fn take_completed_migration(&mut self) -> Option<SessionMigrationPlan> {
        self.completed_migration.take()
    }

    fn begin_tagged_request(
        &mut self,
        body: RequestBody,
        pending: PendingRequest,
    ) -> Result<ClientRequestJob, SessionUiError> {
        if self.pending.is_some() {
            return Err(SessionUiError::RequestInFlight);
        }
        let client = self.client.as_mut().ok_or(SessionUiError::Unavailable)?;
        let job = client.begin_request(body)?;
        self.pending = Some(pending);
        Ok(job)
    }

    pub(crate) fn finish_request(
        &mut self,
        completion: &SessionRequestCompletion,
    ) -> Result<ResponseBody, SessionUiError> {
        let completion = completion.take().ok_or(SessionUiError::Consumed)?;
        let pending = self
            .pending
            .take()
            .ok_or(SessionUiError::NoPendingRequest)?;
        let client = self.client.as_mut().ok_or(SessionUiError::Unavailable)?;
        let response = client.finish_request(completion)?;
        if let (PendingRequest::Snapshot(pane), ResponseBody::PaneSnapshot(snapshot)) =
            (pending, &response)
        {
            let _ = client.apply_snapshot(pane, snapshot.clone());
        }
        if pending == PendingRequest::Migration
            && matches!(response, ResponseBody::CatalogChanged(_))
        {
            self.completed_migration = self.migration_plan.take();
        }
        self.sync_selection();
        Ok(response)
    }

    pub(crate) fn select_session(&mut self, session: SessionId) -> bool {
        let Some((window, pane)) = self.catalog().and_then(|snapshot| {
            let target = snapshot.catalog().session(session)?;
            let window = target.active_window()?;
            Some((window.id(), window.focused_pane().id()))
        }) else {
            return false;
        };
        self.selected_session = Some(session);
        self.selected_window = Some(window);
        self.selected_pane = Some(pane);
        true
    }

    pub(crate) fn select_window(&mut self, window: WindowId) -> bool {
        let Some(session_id) = self.selected_session else {
            return false;
        };
        let Some(pane) = self.catalog().and_then(|snapshot| {
            snapshot
                .catalog()
                .session(session_id)?
                .windows()
                .iter()
                .find(|item| item.id() == window)
                .map(|target| target.focused_pane().id())
        }) else {
            return false;
        };
        self.selected_window = Some(window);
        self.selected_pane = Some(pane);
        true
    }

    pub(crate) fn select_pane(&mut self, pane: PaneId) -> bool {
        let Some(snapshot) = self.catalog() else {
            return false;
        };
        let Some((session, window, _)) = snapshot.catalog().pane(pane) else {
            return false;
        };
        self.selected_session = Some(session);
        self.selected_window = Some(window);
        self.selected_pane = Some(pane);
        true
    }

    fn sync_selection(&mut self) {
        let selected_valid = self.selected_pane.is_some_and(|pane| {
            self.catalog()
                .is_some_and(|snapshot| snapshot.catalog().contains_pane(pane))
        });
        if selected_valid {
            return;
        }
        let selection = self.catalog().and_then(|snapshot| {
            let catalog = snapshot.catalog();
            let session = catalog
                .active_session_id()
                .and_then(|id| catalog.session(id))
                .or_else(|| catalog.sessions().next())?;
            let window = session
                .active_window()
                .or_else(|| session.windows().first())?;
            Some((session.id(), window.id(), window.focused_pane().id()))
        });
        if let Some((session, window, pane)) = selection {
            self.selected_session = Some(session);
            self.selected_window = Some(window);
            self.selected_pane = Some(pane);
        } else {
            self.selected_session = None;
            self.selected_window = None;
            self.selected_pane = None;
        }
    }
}

#[derive(Clone)]
pub(crate) struct SessionConnectCompletion(Arc<Mutex<Option<ClientConnectCompletion>>>);

impl std::fmt::Debug for SessionConnectCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("SessionConnectCompletion").finish()
    }
}

impl SessionConnectCompletion {
    pub(crate) fn new(completion: ClientConnectCompletion) -> Self {
        Self(Arc::new(Mutex::new(Some(completion))))
    }

    fn take(&self) -> Option<ClientConnectCompletion> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

#[derive(Clone)]
pub(crate) struct SessionRequestCompletion(Arc<Mutex<Option<ClientRequestCompletion>>>);

impl std::fmt::Debug for SessionRequestCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("SessionRequestCompletion").finish()
    }
}

impl SessionRequestCompletion {
    pub(crate) fn new(completion: ClientRequestCompletion) -> Self {
        Self(Arc::new(Mutex::new(Some(completion))))
    }

    fn take(&self) -> Option<ClientRequestCompletion> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

fn platform_client() -> Result<SessionClient, SessionUiError> {
    let directories =
        ProjectDirs::from("dev", "strukt", "strukt").ok_or(SessionUiError::Unavailable)?;
    let application_data = directories.data_local_dir().join("sessions");
    let executable = std::env::current_exe()?;
    let helper = executable
        .parent()
        .ok_or(SessionUiError::Unavailable)?
        .join(helper_file_name());
    Ok(SessionClient::new(application_data, helper)?)
}

fn helper_file_name() -> &'static Path {
    #[cfg(windows)]
    return Path::new("strukt-sessiond.exe");
    #[cfg(not(windows))]
    return Path::new("strukt-sessiond");
}

#[derive(Debug, Error)]
pub(crate) enum SessionUiError {
    #[error("persistent session service is unavailable")]
    Unavailable,
    #[error("no persistent session pane is selected")]
    NoSelection,
    #[error("another persistent session request is in flight")]
    RequestInFlight,
    #[error("persistent session request completion was already consumed")]
    Consumed,
    #[error("persistent session request has no matching pending operation")]
    NoPendingRequest,
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Migration(#[from] strukt_persistence::SessionMigrationError),
    #[error("persistent session platform path failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_default_is_lazy_and_has_no_projection() {
        let surfaces = SessionSurfaces::default();
        assert_eq!(surfaces.health(), ClientHealth::Stopped);
        assert!(surfaces.catalog().is_none());
        assert!(!surfaces.request_in_flight());
    }

    #[test]
    fn session_invalid_selection_is_rejected_without_state_change() {
        let mut surfaces = SessionSurfaces::default();
        let session = SessionId::new().expect("session id");
        let pane = PaneId::new().expect("pane id");
        assert!(!surfaces.select_session(session));
        assert!(!surfaces.select_pane(pane));
        assert_eq!(surfaces.selected_session(), None);
        assert_eq!(surfaces.selected_window(), None);
        assert_eq!(surfaces.selected_pane(), None);
    }
}
