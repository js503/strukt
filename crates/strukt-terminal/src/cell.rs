use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_CELL_TEXT_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum CellWidth {
    #[default]
    Single,
    Wide,
    Continuation,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct CellAttributes {
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub strikethrough: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum CellError {
    #[error("continuation cells cannot contain text")]
    ContinuationHasText,
    #[error("cell text exceeds the {MAX_CELL_TEXT_BYTES}-byte bound")]
    TextTooLong,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    text: String,
    width: CellWidth,
    pub foreground: Color,
    pub background: Color,
    pub attributes: CellAttributes,
}

impl Cell {
    /// Replaces the cell's bounded text and display width.
    ///
    /// # Errors
    ///
    /// Returns [`CellError::ContinuationHasText`] when a continuation cell is
    /// given content, or [`CellError::TextTooLong`] when `text` exceeds the
    /// per-cell allocation bound.
    pub fn set_text(&mut self, text: &str, width: CellWidth) -> Result<(), CellError> {
        if width == CellWidth::Continuation && !text.is_empty() {
            return Err(CellError::ContinuationHasText);
        }
        if text.len() > MAX_CELL_TEXT_BYTES {
            return Err(CellError::TextTooLong);
        }

        self.text.clear();
        self.text.push_str(text);
        self.width = width;
        Ok(())
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn width(&self) -> CellWidth {
        self.width
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub fn is_structurally_valid(&self) -> bool {
        self.text.len() <= MAX_CELL_TEXT_BYTES
            && (self.width != CellWidth::Continuation || self.text.is_empty())
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            text: " ".to_owned(),
            width: CellWidth::Single,
            foreground: Color::Default,
            background: Color::Default,
            attributes: CellAttributes::default(),
        }
    }
}
