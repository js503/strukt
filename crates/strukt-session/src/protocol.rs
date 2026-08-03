use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    PaneId, ProviderCatalogSnapshot, ProviderError, ServiceInstanceId, SessionId, WindowId,
};

pub const PROTOCOL_VERSION: u16 = 1;
const MAX_NAME_CHARS: usize = 80;
const MAX_PATH_BYTES: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RequestBody {
    Catalog,
    Attach,
    Detach,
    CreateSession {
        name: String,
        working_directory: PathBuf,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    version: u16,
    request_id: u64,
    expected_catalog_revision: u64,
    body: RequestBody,
}

impl RequestEnvelope {
    #[must_use]
    pub const fn new(request_id: u64, expected_catalog_revision: u64, body: RequestBody) -> Self {
        Self::with_version(
            PROTOCOL_VERSION,
            request_id,
            expected_catalog_revision,
            body,
        )
    }

    #[must_use]
    pub const fn with_version(
        version: u16,
        request_id: u64,
        expected_catalog_revision: u64,
        body: RequestBody,
    ) -> Self {
        Self {
            version,
            request_id,
            expected_catalog_revision,
            body,
        }
    }

    #[must_use]
    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    #[must_use]
    pub const fn expected_catalog_revision(&self) -> u64 {
        self.expected_catalog_revision
    }

    #[must_use]
    pub const fn body(&self) -> &RequestBody {
        &self.body
    }

    /// Validates version and variable-length request values.
    ///
    /// # Errors
    ///
    /// Returns a version or body validation error.
    pub fn validate(&self) -> Result<(), WireError> {
        if self.version != PROTOCOL_VERSION {
            return Err(WireError::VersionIncompatible);
        }
        if self.request_id == 0 {
            return Err(WireError::InvalidRequestId);
        }
        match &self.body {
            RequestBody::CreateSession {
                name,
                working_directory,
            } => {
                let trimmed = name.trim();
                let path_bytes = working_directory.as_os_str().as_encoded_bytes();
                if trimmed.is_empty()
                    || trimmed.chars().count() > MAX_NAME_CHARS
                    || path_bytes.is_empty()
                    || path_bytes.len() > MAX_PATH_BYTES
                {
                    return Err(WireError::InvalidBody);
                }
            }
            RequestBody::Catalog | RequestBody::Attach | RequestBody::Detach => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ResponseBody {
    Catalog(ProviderCatalogSnapshot),
    Attached(ProviderCatalogSnapshot),
    Detached,
    SessionCreated(SessionId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    version: u16,
    request_id: u64,
    result: Result<ResponseBody, ProviderError>,
}

impl ResponseEnvelope {
    #[must_use]
    pub const fn ok(request_id: u64, body: ResponseBody) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            result: Ok(body),
        }
    }

    #[must_use]
    pub const fn error(request_id: u64, error: ProviderError) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            result: Err(error),
        }
    }

    #[must_use]
    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    pub const fn result(&self) -> &Result<ResponseBody, ProviderError> {
        &self.result
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    version: u16,
    service_instance: ServiceInstanceId,
    session: SessionId,
    window: WindowId,
    pane: PaneId,
    generation: u64,
    output_revision: u64,
}

impl EventEnvelope {
    #[must_use]
    pub const fn pane_changed(
        service_instance: ServiceInstanceId,
        session: SessionId,
        window: WindowId,
        pane: PaneId,
        generation: u64,
        output_revision: u64,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            service_instance,
            session,
            window,
            pane,
            generation,
            output_revision,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventGuard {
    service_instance: ServiceInstanceId,
    session: SessionId,
    window: WindowId,
    pane: PaneId,
    generation: u64,
    output_revision: u64,
}

impl EventGuard {
    #[must_use]
    pub const fn new(
        service_instance: ServiceInstanceId,
        session: SessionId,
        window: WindowId,
        pane: PaneId,
        generation: u64,
        output_revision: u64,
    ) -> Self {
        Self {
            service_instance,
            session,
            window,
            pane,
            generation,
            output_revision,
        }
    }

    #[must_use]
    pub fn matches(self, event: &EventEnvelope) -> bool {
        event.version == PROTOCOL_VERSION
            && event.service_instance == self.service_instance
            && event.session == self.session
            && event.window == self.window
            && event.pane == self.pane
            && event.generation == self.generation
            && event.output_revision > self.output_revision
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RequestIdGenerator {
    current: u64,
}

impl RequestIdGenerator {
    #[must_use]
    pub const fn new() -> Self {
        Self { current: 0 }
    }

    /// Returns the next nonzero request identifier.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::RequestIdExhausted`] at `u64::MAX`.
    pub fn next_id(&mut self) -> Result<u64, WireError> {
        self.current = self
            .current
            .checked_add(1)
            .ok_or(WireError::RequestIdExhausted)?;
        Ok(self.current)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum WireError {
    #[error("session protocol version is incompatible")]
    VersionIncompatible,
    #[error("session request identifier is invalid")]
    InvalidRequestId,
    #[error("session request identifier space is exhausted")]
    RequestIdExhausted,
    #[error("session protocol body is invalid")]
    InvalidBody,
}
