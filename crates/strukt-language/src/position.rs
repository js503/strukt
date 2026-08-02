use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScalarPosition {
    pub line: u32,
    pub character: u32,
}

impl ScalarPosition {
    #[must_use]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

impl LspPosition {
    #[must_use]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Converts a Unicode-scalar position into an LSP position.
///
/// # Errors
///
/// Returns an error when the line or scalar column is outside `text`.
pub fn to_lsp_position(
    text: &str,
    position: ScalarPosition,
    encoding: PositionEncoding,
) -> Result<LspPosition, PositionError> {
    let line = line_at(text, position.line)?;
    let scalar_column =
        usize::try_from(position.character).map_err(|_| PositionError::InvalidCharacter)?;
    let byte_column = scalar_to_byte(line, scalar_column)?;
    let character = match encoding {
        PositionEncoding::Utf8 => byte_column,
        PositionEncoding::Utf16 => line[..byte_column].encode_utf16().count(),
    };
    Ok(LspPosition::new(
        position.line,
        u32::try_from(character).map_err(|_| PositionError::InvalidCharacter)?,
    ))
}

/// Converts an LSP position into a Unicode-scalar position.
///
/// # Errors
///
/// Returns an error when the line or encoded column is outside `text`, or when
/// a UTF-16 position lands inside a surrogate pair.
pub fn from_lsp_position(
    text: &str,
    position: LspPosition,
    encoding: PositionEncoding,
) -> Result<ScalarPosition, PositionError> {
    let line = line_at(text, position.line)?;
    let target =
        usize::try_from(position.character).map_err(|_| PositionError::InvalidCharacter)?;
    let byte_column = match encoding {
        PositionEncoding::Utf8 => {
            if target > line.len() || !line.is_char_boundary(target) {
                return Err(PositionError::InvalidCharacter);
            }
            target
        }
        PositionEncoding::Utf16 => utf16_to_byte(line, target)?,
    };
    let scalars = line[..byte_column].chars().count();
    Ok(ScalarPosition::new(
        position.line,
        u32::try_from(scalars).map_err(|_| PositionError::InvalidCharacter)?,
    ))
}

fn line_at(text: &str, target: u32) -> Result<&str, PositionError> {
    let line = text
        .split('\n')
        .nth(usize::try_from(target).map_err(|_| PositionError::InvalidLine)?)
        .ok_or(PositionError::InvalidLine)?;
    Ok(line.strip_suffix('\r').unwrap_or(line))
}

fn scalar_to_byte(line: &str, scalar_column: usize) -> Result<usize, PositionError> {
    if scalar_column == line.chars().count() {
        return Ok(line.len());
    }
    line.char_indices()
        .nth(scalar_column)
        .map(|(index, _)| index)
        .ok_or(PositionError::InvalidCharacter)
}

fn utf16_to_byte(line: &str, target: usize) -> Result<usize, PositionError> {
    if target == 0 {
        return Ok(0);
    }
    let mut units = 0;
    for (byte_index, character) in line.char_indices() {
        let next = units + character.len_utf16();
        if target == units {
            return Ok(byte_index);
        }
        if target < next {
            return Err(PositionError::InvalidCharacter);
        }
        units = next;
    }
    if target == units {
        Ok(line.len())
    } else {
        Err(PositionError::InvalidCharacter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PositionError {
    #[error("line is outside the document")]
    InvalidLine,
    #[error("character is outside the line or not on an encoding boundary")]
    InvalidCharacter,
}
