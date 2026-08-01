use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;
use thiserror::Error;

use crate::{CellWidth, HyperlinkId, TerminalModel, TerminalSnapshot};

const ORDINARY_PASTE_LIMIT: usize = 1024 * 1024;
const LINK_SCAN_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TerminalCoordinate {
    pub row: usize,
    pub column: usize,
}

impl From<(usize, usize)> for TerminalCoordinate {
    fn from((row, column): (usize, usize)) -> Self {
        Self { row, column }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection {
    start: TerminalCoordinate,
    end: TerminalCoordinate,
}

impl Selection {
    #[must_use]
    pub fn linear(
        start: impl Into<TerminalCoordinate>,
        end: impl Into<TerminalCoordinate>,
    ) -> Self {
        let start = start.into();
        let end = end.into();
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    #[must_use]
    pub const fn start(self) -> TerminalCoordinate {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> TerminalCoordinate {
        self.end
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum SelectionError {
    #[error("terminal selection is outside the visible snapshot")]
    OutOfBounds,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PasteDecision {
    Send(Vec<u8>),
    Confirm { bytes: Vec<u8>, bracketed: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKey {
    ArrowUp,
    ArrowDown,
    ArrowRight,
    ArrowLeft,
    Home,
    End,
    Enter,
    Backspace,
    Tab,
    Escape,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusEvent {
    In,
    Out,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MouseEvent {
    column: usize,
    row: usize,
    button: MouseButton,
    pressed: bool,
}

impl MouseEvent {
    #[must_use]
    pub const fn press(column: usize, row: usize, button: MouseButton) -> Self {
        Self {
            column,
            row,
            button,
            pressed: true,
        }
    }

    #[must_use]
    pub const fn release(column: usize, row: usize, button: MouseButton) -> Self {
        Self {
            column,
            row,
            button,
            pressed: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LinkId(u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalLink {
    id: LinkId,
    target: String,
    opened: bool,
}

impl TerminalLink {
    #[must_use]
    pub const fn id(&self) -> LinkId {
        self.id
    }

    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    #[must_use]
    pub const fn opened(&self) -> bool {
        self.opened
    }
}

impl TerminalModel {
    /// Copies the selected visible cells with wide-cell continuation snapping.
    ///
    /// # Errors
    ///
    /// Returns [`SelectionError::OutOfBounds`] when either endpoint is outside
    /// the current visible snapshot.
    pub fn copy_text(&self, selection: &Selection) -> Result<String, SelectionError> {
        copy_text(&self.snapshot(0), selection)
    }

    #[must_use]
    pub fn links(&self) -> std::vec::IntoIter<TerminalLink> {
        discover_links(&self.snapshot(0)).into_iter()
    }

    #[must_use]
    pub fn prepare_paste(&self, text: &str, confirmed: bool) -> PasteDecision {
        let clean = text.replace('\0', "");
        let bracketed = self.snapshot(0).modes().bracketed_paste;
        if clean.len() > ORDINARY_PASTE_LIMIT && !confirmed {
            return PasteDecision::Confirm {
                bytes: clean.into_bytes(),
                bracketed,
            };
        }

        let mut bytes = Vec::with_capacity(clean.len() + usize::from(bracketed) * 12);
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(clean.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        PasteDecision::Send(bytes)
    }

    #[must_use]
    pub fn encode_key(&self, key: TerminalKey) -> Vec<u8> {
        let application = self.snapshot(0).modes().application_cursor_keys;
        match (key, application) {
            (TerminalKey::ArrowUp, true) => b"\x1bOA".to_vec(),
            (TerminalKey::ArrowDown, true) => b"\x1bOB".to_vec(),
            (TerminalKey::ArrowRight, true) => b"\x1bOC".to_vec(),
            (TerminalKey::ArrowLeft, true) => b"\x1bOD".to_vec(),
            (TerminalKey::ArrowUp, false) => b"\x1b[A".to_vec(),
            (TerminalKey::ArrowDown, false) => b"\x1b[B".to_vec(),
            (TerminalKey::ArrowRight, false) => b"\x1b[C".to_vec(),
            (TerminalKey::ArrowLeft, false) => b"\x1b[D".to_vec(),
            (TerminalKey::Home, _) => b"\x1b[H".to_vec(),
            (TerminalKey::End, _) => b"\x1b[F".to_vec(),
            (TerminalKey::Enter, _) => b"\r".to_vec(),
            (TerminalKey::Backspace, _) => vec![0x7f],
            (TerminalKey::Tab, _) => b"\t".to_vec(),
            (TerminalKey::Escape, _) => vec![0x1b],
        }
    }

    #[must_use]
    pub fn encode_focus(&self, event: FocusEvent) -> Option<Vec<u8>> {
        self.snapshot(0)
            .modes()
            .focus_reporting
            .then(|| match event {
                FocusEvent::In => b"\x1b[I".to_vec(),
                FocusEvent::Out => b"\x1b[O".to_vec(),
            })
    }

    #[must_use]
    pub fn encode_mouse(&self, event: MouseEvent) -> Option<Vec<u8>> {
        if !self.snapshot(0).modes().mouse_reporting {
            return None;
        }
        let button = match event.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        };
        let suffix = if event.pressed { 'M' } else { 'm' };
        Some(
            format!(
                "\x1b[<{button};{};{}{suffix}",
                event.column.saturating_add(1),
                event.row.saturating_add(1)
            )
            .into_bytes(),
        )
    }
}

fn copy_text(snapshot: &TerminalSnapshot, selection: &Selection) -> Result<String, SelectionError> {
    let rows = snapshot.rows();
    let Some(start_row) = rows.get(selection.start.row) else {
        return Err(SelectionError::OutOfBounds);
    };
    let Some(end_row) = rows.get(selection.end.row) else {
        return Err(SelectionError::OutOfBounds);
    };
    if selection.start.column >= start_row.len() || selection.end.column >= end_row.len() {
        return Err(SelectionError::OutOfBounds);
    }

    let mut copied = String::new();
    for (row_index, row) in rows
        .iter()
        .enumerate()
        .take(selection.end.row + 1)
        .skip(selection.start.row)
    {
        if row_index > selection.start.row {
            copied.push('\n');
        }
        let start = if row_index == selection.start.row {
            selection.start.column
        } else {
            0
        };
        let end = if row_index == selection.end.row {
            selection.end.column
        } else {
            row.len() - 1
        };
        for cell in &row[start..=end] {
            if cell.width() != CellWidth::Continuation {
                copied.push_str(cell.text());
            }
        }
    }
    Ok(copied)
}

fn discover_links(snapshot: &TerminalSnapshot) -> Vec<TerminalLink> {
    let mut targets = Vec::new();
    let mut explicit_ids = BTreeSet::<HyperlinkId>::new();
    for cell in snapshot.rows().iter().flatten() {
        if let Some(id) = cell.hyperlink
            && explicit_ids.insert(id)
            && let Some(target) = snapshot.hyperlink_target(id)
        {
            targets.push(target.to_owned());
        }
    }

    let visible = bounded_visible_text(snapshot);
    for matched in link_regex().find_iter(&visible) {
        let target = matched
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', ')', ']', '}']);
        if !targets.iter().any(|existing| existing == target) {
            targets.push(target.to_owned());
        }
    }

    targets
        .into_iter()
        .enumerate()
        .map(|(index, target)| TerminalLink {
            id: LinkId(u32::try_from(index).unwrap_or(u32::MAX)),
            target,
            opened: false,
        })
        .collect()
}

fn bounded_visible_text(snapshot: &TerminalSnapshot) -> String {
    let mut text = String::new();
    for cell in snapshot.rows().iter().flatten() {
        if cell.width() != CellWidth::Continuation {
            let remaining = LINK_SCAN_LIMIT.saturating_sub(text.len());
            if remaining == 0 {
                break;
            }
            let content = cell.text();
            let boundary = content
                .char_indices()
                .take_while(|(index, _)| *index < remaining)
                .map(|(index, character)| index + character.len_utf8())
                .last()
                .unwrap_or(0);
            text.push_str(&content[..boundary]);
        }
    }
    text
}

fn link_regex() -> &'static Regex {
    static LINK_REGEX: OnceLock<Regex> = OnceLock::new();
    LINK_REGEX.get_or_init(|| {
        Regex::new(r#"(?i)(?:https?://|file://|mailto:)[^\s<>"']+"#)
            .expect("the terminal URL pattern is static and valid")
    })
}
