use std::io::Cursor;

use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

const HEADER_BYTES: usize = 4;

#[derive(Clone, Debug)]
pub struct FrameDecoder {
    retained: Vec<u8>,
    max_payload_bytes: usize,
}

impl FrameDecoder {
    #[must_use]
    pub const fn new(max_payload_bytes: usize) -> Self {
        Self {
            retained: Vec::new(),
            max_payload_bytes,
        }
    }

    /// Pushes bytes and returns every complete payload.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::FrameTooLarge`] and clears retained input when a
    /// declared or buffered frame exceeds the configured bound.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        let maximum_retained = self
            .max_payload_bytes
            .checked_add(HEADER_BYTES)
            .ok_or(FrameError::FrameTooLarge)?;
        if bytes.len() > maximum_retained
            || (self.retained.len().saturating_add(bytes.len()) > maximum_retained
                && self.retained.len() < HEADER_BYTES)
        {
            self.retained.clear();
            return Err(FrameError::FrameTooLarge);
        }
        self.retained.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            if self.retained.len() < HEADER_BYTES {
                break;
            }
            let header: [u8; HEADER_BYTES] = self.retained[..HEADER_BYTES]
                .try_into()
                .map_err(|_| FrameError::MalformedPayload)?;
            let length = u32::from_be_bytes(header) as usize;
            if length > self.max_payload_bytes {
                self.retained.clear();
                return Err(FrameError::FrameTooLarge);
            }
            let complete_length = HEADER_BYTES
                .checked_add(length)
                .ok_or(FrameError::FrameTooLarge)?;
            if self.retained.len() < complete_length {
                break;
            }
            frames.push(self.retained[HEADER_BYTES..complete_length].to_vec());
            self.retained.drain(..complete_length);
        }
        Ok(frames)
    }

    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.retained.len()
    }
}

/// Encodes one typed CBOR frame with a big-endian length prefix.
///
/// # Errors
///
/// Returns a serialization or size error.
pub fn encode_cbor<T: Serialize>(
    value: &T,
    max_payload_bytes: usize,
) -> Result<Vec<u8>, FrameError> {
    let mut payload = Vec::new();
    ciborium::into_writer(value, &mut payload).map_err(|_| FrameError::MalformedPayload)?;
    if payload.len() > max_payload_bytes || payload.len() > u32::MAX as usize {
        return Err(FrameError::FrameTooLarge);
    }
    let mut frame = Vec::with_capacity(HEADER_BYTES + payload.len());
    let payload_length = u32::try_from(payload.len()).map_err(|_| FrameError::FrameTooLarge)?;
    frame.extend_from_slice(&payload_length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decodes one typed CBOR payload and rejects trailing values.
///
/// # Errors
///
/// Returns [`FrameError::MalformedPayload`] for invalid or ambiguous input.
pub fn decode_cbor<T: DeserializeOwned>(payload: &[u8]) -> Result<T, FrameError> {
    let mut reader = Cursor::new(payload);
    let value = ciborium::de::from_reader_with_recursion_limit(&mut reader, 32)
        .map_err(|_| FrameError::MalformedPayload)?;
    if reader.position() != payload.len() as u64 {
        return Err(FrameError::MalformedPayload);
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum FrameError {
    #[error("session protocol frame exceeds its configured bound")]
    FrameTooLarge,
    #[error("session protocol payload is malformed")]
    MalformedPayload,
}
