use std::io::{self, Cursor, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

const PREFACE: &[u8] = b"STRUKT-REMOTE\0\x01";
pub const DEFAULT_FRAME_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum FramingError {
    #[error("remote helper stream ended")]
    EndOfStream,
    #[error("remote helper preface is invalid")]
    InvalidPreface,
    #[error("remote helper frame length is invalid")]
    InvalidLength,
    #[error("remote helper frame contains invalid CBOR")]
    InvalidCbor,
    #[error("remote helper frame contains trailing data")]
    TrailingData,
    #[error("remote helper I/O failed: {0}")]
    Io(#[from] io::Error),
}

/// Writes the fixed protocol preface.
///
/// # Errors
///
/// Returns [`FramingError::Io`] when the writer cannot accept the full preface.
pub fn write_preface(writer: &mut impl Write) -> Result<(), FramingError> {
    writer.write_all(PREFACE)?;
    Ok(())
}

/// Reads and validates the fixed protocol preface.
///
/// # Errors
///
/// Returns [`FramingError::InvalidPreface`] for any mismatch and
/// [`FramingError::Io`] for a truncated stream.
pub fn read_preface(reader: &mut impl Read) -> Result<(), FramingError> {
    let mut preface = vec![0_u8; PREFACE.len()];
    reader.read_exact(&mut preface)?;
    if preface == PREFACE {
        Ok(())
    } else {
        Err(FramingError::InvalidPreface)
    }
}

/// Serializes and writes one bounded length-prefixed CBOR frame.
///
/// # Errors
///
/// Returns a framing error for serialization, length, conversion, or I/O failure.
pub fn write_frame<T: Serialize>(
    writer: &mut impl Write,
    value: &T,
    maximum: usize,
) -> Result<(), FramingError> {
    let mut payload = Vec::new();
    ciborium::into_writer(value, &mut payload).map_err(|_| FramingError::InvalidCbor)?;
    if payload.is_empty() || payload.len() > maximum {
        return Err(FramingError::InvalidLength);
    }
    let length = u32::try_from(payload.len()).map_err(|_| FramingError::InvalidLength)?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload)?;
    Ok(())
}

/// Reads and deserializes one bounded length-prefixed CBOR frame.
///
/// # Errors
///
/// Returns [`FramingError::EndOfStream`] only for clean EOF before a new length;
/// truncated, oversized, invalid, or trailing data produce distinct errors.
pub fn read_frame<R: Read, T: DeserializeOwned>(
    reader: &mut R,
    maximum: usize,
) -> Result<T, FramingError> {
    let mut length_bytes = [0_u8; 4];
    match reader.read(&mut length_bytes[..1]) {
        Ok(0) => return Err(FramingError::EndOfStream),
        Ok(_) => {}
        Err(error) => return Err(FramingError::Io(error)),
    }
    reader.read_exact(&mut length_bytes[1..])?;
    let length = usize::try_from(u32::from_be_bytes(length_bytes))
        .map_err(|_| FramingError::InvalidLength)?;
    if length == 0 || length > maximum {
        return Err(FramingError::InvalidLength);
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    let mut cursor = Cursor::new(payload);
    let value = ciborium::from_reader(&mut cursor).map_err(|_| FramingError::InvalidCbor)?;
    if usize::try_from(cursor.position()).ok() != Some(length) {
        return Err(FramingError::TrailingData);
    }
    Ok(value)
}
