use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_CELL_TEXT_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct HyperlinkId(pub(crate) u32);

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
    pub hyperlink: Option<HyperlinkId>,
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

    pub(crate) fn append_combining(&mut self, character: char) -> Result<(), CellError> {
        if self.width == CellWidth::Continuation {
            return Err(CellError::ContinuationHasText);
        }
        if self.text.len() + character.len_utf8() > MAX_CELL_TEXT_BYTES {
            return Err(CellError::TextTooLong);
        }
        if self.text == " " {
            self.text.clear();
        }
        self.text.push(character);
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

    pub(crate) fn is_semantically_blank(&self) -> bool {
        self.text == " "
            && self.width == CellWidth::Single
            && self.foreground == Color::Default
            && self.background == Color::Default
            && self.attributes == CellAttributes::default()
            && self.hyperlink.is_none()
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
            hyperlink: None,
        }
    }
}
