use std::collections::HashSet;

use serde_json::Value;
use thiserror::Error;

const OUTSTANDING_REQUEST_LIMIT: usize = 256;
const ERROR_TEXT_LIMIT: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(u64);

impl RequestId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Default)]
pub struct RequestIdAllocator {
    next: u64,
}

impl RequestIdAllocator {
    /// Allocates the next positive numeric request identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::RequestIdExhausted`] after `u64::MAX`.
    pub fn next_id(&mut self) -> Result<RequestId, ProtocolError> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or(ProtocolError::RequestIdExhausted)?;
        Ok(RequestId(self.next))
    }
}

#[derive(Debug, Default)]
pub struct ResponseRouter {
    outstanding: HashSet<RequestId>,
}

impl ResponseRouter {
    /// Registers a request ID before its message is sent.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate IDs or when the outstanding-request limit
    /// has been reached.
    pub fn register(&mut self, id: RequestId) -> Result<(), ProtocolError> {
        if self.outstanding.contains(&id) {
            return Err(ProtocolError::DuplicateRequest { id: id.get() });
        }
        if self.outstanding.len() >= OUTSTANDING_REQUEST_LIMIT {
            return Err(ProtocolError::TooManyOutstandingRequests);
        }
        self.outstanding.insert(id);
        Ok(())
    }

    /// Accepts exactly one response for a registered request ID.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate or otherwise unexpected responses.
    pub fn accept(&mut self, response: &ResponseMessage) -> Result<(), ProtocolError> {
        if self.outstanding.remove(&response.id()) {
            Ok(())
        } else {
            Err(ProtocolError::UnexpectedResponse {
                id: response.id().get(),
            })
        }
    }
}

#[must_use]
pub fn bounded_error_text(text: &str) -> String {
    if text.len() <= ERROR_TEXT_LIMIT {
        return text.to_owned();
    }
    let suffix = '…';
    let mut end = ERROR_TEXT_LIMIT - suffix.len_utf8();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = String::with_capacity(ERROR_TEXT_LIMIT);
    bounded.push_str(&text[..end]);
    bounded.push(suffix);
    bounded
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResponseMessage {
    id: RequestId,
    result: Option<Value>,
    error: Option<Value>,
}

impl ResponseMessage {
    #[must_use]
    pub const fn id(&self) -> RequestId {
        self.id
    }

    #[must_use]
    pub const fn result(&self) -> Option<&Value> {
        self.result.as_ref()
    }

    #[must_use]
    pub const fn error(&self) -> Option<&Value> {
        self.error.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotificationMessage {
    method: String,
    params: Option<Value>,
}

impl NotificationMessage {
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    #[must_use]
    pub const fn params(&self) -> Option<&Value> {
        self.params.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RequestMessage {
    id: RequestId,
    method: String,
    params: Option<Value>,
}

impl RequestMessage {
    #[must_use]
    pub const fn id(&self) -> RequestId {
        self.id
    }

    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    #[must_use]
    pub const fn params(&self) -> Option<&Value> {
        self.params.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum IncomingMessage {
    Response(ResponseMessage),
    Notification(NotificationMessage),
    Request(RequestMessage),
}

/// Parses and classifies one JSON-RPC 2.0 message from an LSP frame body.
///
/// # Errors
///
/// Returns [`ProtocolError::InvalidJson`] for invalid JSON and
/// [`ProtocolError::InvalidMessage`] for an invalid or ambiguous envelope.
pub fn parse_message(body: &[u8]) -> Result<IncomingMessage, ProtocolError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| ProtocolError::InvalidJson)?;
    let object = value.as_object().ok_or(ProtocolError::InvalidMessage)?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(ProtocolError::InvalidMessage);
    }

    let id = object.get("id").map(parse_request_id).transpose()?;
    let method = object.get("method").and_then(Value::as_str);
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");

    match (id, method, has_result, has_error) {
        (Some(id), None, true, false) => Ok(IncomingMessage::Response(ResponseMessage {
            id,
            result: object.get("result").cloned(),
            error: None,
        })),
        (Some(id), None, false, true) => Ok(IncomingMessage::Response(ResponseMessage {
            id,
            result: None,
            error: object.get("error").cloned(),
        })),
        (Some(id), Some(method), false, false) => Ok(IncomingMessage::Request(RequestMessage {
            id,
            method: method.to_owned(),
            params: object.get("params").cloned(),
        })),
        (None, Some(method), false, false) => {
            Ok(IncomingMessage::Notification(NotificationMessage {
                method: method.to_owned(),
                params: object.get("params").cloned(),
            }))
        }
        _ => Err(ProtocolError::InvalidMessage),
    }
}

fn parse_request_id(value: &Value) -> Result<RequestId, ProtocolError> {
    value
        .as_u64()
        .map(RequestId)
        .ok_or(ProtocolError::InvalidMessage)
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProtocolError {
    #[error("message body is not valid JSON")]
    InvalidJson,
    #[error("message is not an unambiguous JSON-RPC 2.0 message")]
    InvalidMessage,
    #[error("numeric request identifiers are exhausted")]
    RequestIdExhausted,
    #[error("request identifier {id} was already registered")]
    DuplicateRequest { id: u64 },
    #[error("response identifier {id} is duplicate or unknown")]
    UnexpectedResponse { id: u64 },
    #[error("outstanding request limit reached")]
    TooManyOutstandingRequests,
}
