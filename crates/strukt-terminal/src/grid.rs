use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroUsize;

use thiserror::Error;
use unicode_width::UnicodeWidthChar;

use crate::{Cell, CellAttributes, CellWidth, Color, HyperlinkId};

const MAX_SCROLLBACK_ROWS: usize = 100_000;

pub type Row = Vec<Cell>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridSize {
    rows: NonZeroUsize,
    columns: NonZeroUsize,
}

impl GridSize {
    /// Creates a nonempty terminal grid size.
    ///
    /// # Errors
    ///
    /// Returns [`GridError::EmptyDimension`] when either dimension is zero.
    pub fn new(rows: usize, columns: usize) -> Result<Self, GridError> {
        Ok(Self {
            rows: NonZeroUsize::new(rows).ok_or(GridError::EmptyDimension)?,
            columns: NonZeroUsize::new(columns).ok_or(GridError::EmptyDimension)?,
        })
    }

    #[must_use]
    pub const fn rows(self) -> usize {
        self.rows.get()
    }

    #[must_use]
    pub const fn columns(self) -> usize {
        self.columns.get()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum GridError {
    #[error("terminal grid dimensions must be nonzero")]
    EmptyDimension,
    #[error("scroll region must be ordered and fit inside the grid")]
    InvalidScrollRegion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EraseDisplay {
    Below,
    Above,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EraseLine {
    Right,
    Left,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizeOutcome {
    changed: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct TerminalModes {
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
    pub mouse_reporting: bool,
}

impl ResizeOutcome {
    #[must_use]
    pub const fn changed(self) -> bool {
        self.changed
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub row: usize,
    pub column: usize,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveScreen {
    Primary,
    Alternate,
}

#[derive(Clone, Debug)]
struct Screen {
    rows: Vec<Row>,
    cursor: Cursor,
    wrap_pending: bool,
    scroll_top: usize,
    scroll_bottom: usize,
    saved_cursor: Option<Cursor>,
}

impl Screen {
    fn new(size: GridSize) -> Self {
        Self {
            rows: blank_rows(size),
            cursor: Cursor {
                visible: true,
                ..Cursor::default()
            },
            wrap_pending: false,
            scroll_top: 0,
            scroll_bottom: size.rows() - 1,
            saved_cursor: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TerminalSnapshot {
    rows: Vec<Row>,
    cursor: Cursor,
    revision: u64,
    viewport_offset: usize,
    title: Option<String>,
    modes: TerminalModes,
    hyperlink_targets: BTreeMap<HyperlinkId, String>,
}

impl TerminalSnapshot {
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    #[must_use]
    pub fn cell(&self, row: usize, column: usize) -> Option<&Cell> {
        self.rows.get(row)?.get(column)
    }

    #[must_use]
    pub const fn modes(&self) -> TerminalModes {
        self.modes
    }

    #[must_use]
    pub fn hyperlink_target(&self, id: HyperlinkId) -> Option<&str> {
        self.hyperlink_targets.get(&id).map(String::as_str)
    }

    pub(crate) fn set_metadata(
        &mut self,
        title: Option<String>,
        modes: TerminalModes,
        hyperlink_targets: BTreeMap<HyperlinkId, String>,
    ) {
        self.title = title;
        self.modes = modes;
        self.hyperlink_targets = hyperlink_targets;
    }

    #[must_use]
    pub fn plain_text(&self) -> String {
        self.rows
            .iter()
            .map(|row| {
                row.iter()
                    .filter(|cell| cell.width() != CellWidth::Continuation)
                    .map(Cell::text)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Clone, Debug)]
pub struct Grid {
    size: GridSize,
    primary: Screen,
    alternate: Screen,
    active: ActiveScreen,
    scrollback: VecDeque<Row>,
    scrollback_limit: usize,
    revision: u64,
}

impl Grid {
    #[must_use]
    pub fn new(size: GridSize, scrollback_limit: usize) -> Self {
        Self {
            size,
            primary: Screen::new(size),
            alternate: Screen::new(size),
            active: ActiveScreen::Primary,
            scrollback: VecDeque::with_capacity(scrollback_limit.min(MAX_SCROLLBACK_ROWS)),
            scrollback_limit: scrollback_limit.min(MAX_SCROLLBACK_ROWS),
            revision: 0,
        }
    }

    pub fn print(&mut self, text: &str) {
        for character in text.chars() {
            match character {
                '\r' => {
                    let screen = self.active_mut();
                    screen.cursor.column = 0;
                    screen.wrap_pending = false;
                }
                '\n' => self.line_feed(),
                '\u{8}' => {
                    let screen = self.active_mut();
                    screen.cursor.column = screen.cursor.column.saturating_sub(1);
                    screen.wrap_pending = false;
                }
                '\t' => {
                    let spaces = 8 - (self.active_ref().cursor.column % 8);
                    for _ in 0..spaces {
                        self.write_character(
                            ' ',
                            Color::Default,
                            Color::Default,
                            CellAttributes::default(),
                            None,
                        );
                    }
                }
                _ => self.write_character(
                    character,
                    Color::Default,
                    Color::Default,
                    CellAttributes::default(),
                    None,
                ),
            }
        }
    }

    #[must_use]
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    pub fn enter_alternate_screen(&mut self) {
        self.alternate = Screen::new(self.size);
        self.active = ActiveScreen::Alternate;
        self.bump_revision();
    }

    pub fn leave_alternate_screen(&mut self) {
        self.active = ActiveScreen::Primary;
        self.bump_revision();
    }

    #[must_use]
    pub fn snapshot(&self, viewport_offset: usize) -> TerminalSnapshot {
        let screen = self.active_ref();
        let maximum_offset = if self.active == ActiveScreen::Primary {
            self.scrollback.len()
        } else {
            0
        };
        let viewport_offset = viewport_offset.min(maximum_offset);
        let rows = if viewport_offset == 0 {
            screen.rows.clone()
        } else {
            let combined = self
                .scrollback
                .iter()
                .chain(&screen.rows)
                .cloned()
                .collect::<Vec<_>>();
            let end = combined.len() - viewport_offset;
            let start = end.saturating_sub(self.size.rows());
            combined[start..end].to_vec()
        };
        TerminalSnapshot {
            rows,
            cursor: screen.cursor,
            revision: self.revision,
            viewport_offset,
            title: None,
            modes: TerminalModes::default(),
            hyperlink_targets: BTreeMap::new(),
        }
    }

    pub fn set_cursor(&mut self, row: usize, column: usize) {
        let max_row = self.size.rows() - 1;
        let max_column = self.size.columns() - 1;
        let screen = self.active_mut();
        screen.cursor.row = row.min(max_row);
        screen.cursor.column = column.min(max_column);
        screen.wrap_pending = false;
        self.bump_revision();
    }

    #[must_use]
    pub fn cursor(&self) -> Cursor {
        self.active_ref().cursor
    }

    #[must_use]
    pub const fn size(&self) -> GridSize {
        self.size
    }

    pub fn move_cursor(&mut self, row_delta: isize, column_delta: isize) {
        let cursor = self.cursor();
        self.set_cursor(
            cursor.row.saturating_add_signed(row_delta),
            cursor.column.saturating_add_signed(column_delta),
        );
    }

    pub fn save_cursor(&mut self) {
        let cursor = self.active_ref().cursor;
        self.active_mut().saved_cursor = Some(cursor);
    }

    pub fn restore_cursor(&mut self) {
        if let Some(cursor) = self.active_ref().saved_cursor {
            self.active_mut().cursor = cursor;
            self.active_mut().wrap_pending = false;
            self.bump_revision();
        }
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.active_mut().cursor.visible = visible;
        self.bump_revision();
    }

    pub fn resize(&mut self, size: GridSize) -> ResizeOutcome {
        if size == self.size {
            return ResizeOutcome { changed: false };
        }

        resize_screen(&mut self.primary, size);
        resize_screen(&mut self.alternate, size);
        for row in &mut self.scrollback {
            resize_row(row, size.columns());
        }
        self.size = size;
        self.bump_revision();
        ResizeOutcome { changed: true }
    }

    /// Sets the inclusive vertical scrolling region.
    ///
    /// # Errors
    ///
    /// Returns [`GridError::InvalidScrollRegion`] unless `top <= bottom` and
    /// both rows fit inside the visible grid.
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) -> Result<(), GridError> {
        if top > bottom || bottom >= self.size.rows() {
            return Err(GridError::InvalidScrollRegion);
        }
        let screen = self.active_mut();
        screen.scroll_top = top;
        screen.scroll_bottom = bottom;
        screen.cursor.row = top;
        screen.cursor.column = 0;
        screen.wrap_pending = false;
        self.bump_revision();
        Ok(())
    }

    pub fn erase_in_display(&mut self, mode: EraseDisplay) {
        let columns = self.size.columns();
        let screen = self.active_mut();
        let row = screen.cursor.row;
        let column = screen.cursor.column;
        match mode {
            EraseDisplay::Below => {
                erase_range(&mut screen.rows[row], column, columns);
                for visible_row in &mut screen.rows[row + 1..] {
                    *visible_row = blank_row(columns);
                }
            }
            EraseDisplay::Above => {
                for visible_row in &mut screen.rows[..row] {
                    *visible_row = blank_row(columns);
                }
                erase_range(&mut screen.rows[row], 0, column + 1);
            }
            EraseDisplay::All => {
                for visible_row in &mut screen.rows {
                    *visible_row = blank_row(columns);
                }
            }
        }
        screen.wrap_pending = false;
        self.bump_revision();
    }

    pub fn erase_in_line(&mut self, mode: EraseLine) {
        let columns = self.size.columns();
        let screen = self.active_mut();
        let column = screen.cursor.column;
        let row = &mut screen.rows[screen.cursor.row];
        match mode {
            EraseLine::Right => erase_range(row, column, columns),
            EraseLine::Left => erase_range(row, 0, column + 1),
            EraseLine::All => erase_range(row, 0, columns),
        }
        screen.wrap_pending = false;
        self.bump_revision();
    }

    pub fn insert_blank_characters(&mut self, count: usize) {
        let columns = self.size.columns();
        let screen = self.active_mut();
        let column = screen.cursor.column;
        let count = count.max(1).min(columns - column);
        let row = &mut screen.rows[screen.cursor.row];
        row.splice(column..column, std::iter::repeat_n(Cell::default(), count));
        row.truncate(columns);
        repair_row(row);
        screen.wrap_pending = false;
        self.bump_revision();
    }

    pub fn delete_characters(&mut self, count: usize) {
        let columns = self.size.columns();
        let screen = self.active_mut();
        let column = screen.cursor.column;
        let count = count.max(1).min(columns - column);
        let row = &mut screen.rows[screen.cursor.row];
        row.drain(column..column + count);
        row.extend(std::iter::repeat_n(Cell::default(), count));
        repair_row(row);
        screen.wrap_pending = false;
        self.bump_revision();
    }

    pub fn insert_blank_lines(&mut self, count: usize) {
        let columns = self.size.columns();
        let screen = self.active_mut();
        let row = screen.cursor.row;
        if row < screen.scroll_top || row > screen.scroll_bottom {
            return;
        }
        let count = count.max(1).min(screen.scroll_bottom - row + 1);
        for _ in 0..count {
            screen.rows.insert(row, blank_row(columns));
            screen.rows.remove(screen.scroll_bottom + 1);
        }
        screen.wrap_pending = false;
        self.bump_revision();
    }

    pub fn delete_lines(&mut self, count: usize) {
        let columns = self.size.columns();
        let screen = self.active_mut();
        let row = screen.cursor.row;
        if row < screen.scroll_top || row > screen.scroll_bottom {
            return;
        }
        let count = count.max(1).min(screen.scroll_bottom - row + 1);
        for _ in 0..count {
            screen.rows.remove(row);
            screen.rows.insert(screen.scroll_bottom, blank_row(columns));
        }
        screen.wrap_pending = false;
        self.bump_revision();
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let lines = lines.max(1);
        for _ in 0..lines {
            self.scroll_region_up_one();
        }
        self.bump_revision();
    }

    pub fn reverse_index(&mut self) {
        let columns = self.size.columns();
        let screen = self.active_mut();
        if screen.cursor.row == screen.scroll_top {
            screen.rows.remove(screen.scroll_bottom);
            screen.rows.insert(screen.scroll_top, blank_row(columns));
        } else {
            screen.cursor.row = screen.cursor.row.saturating_sub(1);
        }
        screen.wrap_pending = false;
        self.bump_revision();
    }

    pub(crate) fn print_styled(
        &mut self,
        character: char,
        foreground: Color,
        background: Color,
        attributes: CellAttributes,
        hyperlink: Option<HyperlinkId>,
    ) {
        self.write_character(character, foreground, background, attributes, hyperlink);
    }

    fn write_character(
        &mut self,
        character: char,
        foreground: Color,
        background: Color,
        attributes: CellAttributes,
        hyperlink: Option<HyperlinkId>,
    ) {
        let width = UnicodeWidthChar::width(character).unwrap_or(0).min(2);
        if width == 0 {
            self.append_combining(character);
            return;
        }

        let columns = self.size.columns();
        if self.active_ref().wrap_pending
            || (width == 2 && self.active_ref().cursor.column + width > columns)
        {
            self.carriage_return();
            self.line_feed();
        }

        let width = if width == 2 && columns == 1 { 1 } else { width };
        let row = self.active_ref().cursor.row;
        let column = self.active_ref().cursor.column;
        let cell_width = if width == 2 {
            CellWidth::Wide
        } else {
            CellWidth::Single
        };
        let rendered = if width == 1 && UnicodeWidthChar::width(character) == Some(2) {
            '�'
        } else {
            character
        };

        let screen = self.active_mut();
        screen.rows[row][column]
            .set_text(&rendered.to_string(), cell_width)
            .expect("one terminal character fits the cell text bound");
        screen.rows[row][column].foreground = foreground;
        screen.rows[row][column].background = background;
        screen.rows[row][column].attributes = attributes;
        screen.rows[row][column].hyperlink = hyperlink;
        if width == 2 {
            screen.rows[row][column + 1]
                .set_text("", CellWidth::Continuation)
                .expect("empty continuation cell is valid");
            screen.rows[row][column + 1].foreground = foreground;
            screen.rows[row][column + 1].background = background;
            screen.rows[row][column + 1].attributes = attributes;
            screen.rows[row][column + 1].hyperlink = hyperlink;
        }

        let next_column = column + width;
        if next_column == columns {
            screen.cursor.column = columns - 1;
            screen.wrap_pending = true;
        } else {
            screen.cursor.column = next_column;
            screen.wrap_pending = false;
        }
        self.bump_revision();
    }

    fn append_combining(&mut self, character: char) {
        let screen = self.active_mut();
        let row = screen.cursor.row;
        let column = if screen.wrap_pending {
            screen.cursor.column
        } else {
            screen.cursor.column.saturating_sub(1)
        };
        let target = if screen.rows[row][column].width() == CellWidth::Continuation {
            column.saturating_sub(1)
        } else {
            column
        };
        if screen.rows[row][target].append_combining(character).is_ok() {
            self.bump_revision();
        }
    }

    fn carriage_return(&mut self) {
        let screen = self.active_mut();
        screen.cursor.column = 0;
        screen.wrap_pending = false;
    }

    fn line_feed(&mut self) {
        let (row, scroll_bottom) = {
            let screen = self.active_ref();
            (screen.cursor.row, screen.scroll_bottom)
        };
        if row == scroll_bottom {
            self.scroll_region_up_one();
        } else if row < self.size.rows() - 1 {
            self.active_mut().cursor.row += 1;
        }
        self.active_mut().wrap_pending = false;
        self.bump_revision();
    }

    fn scroll_region_up_one(&mut self) {
        let columns = self.size.columns();
        let active = self.active;
        let (removed, is_full_screen) = {
            let screen = self.active_mut();
            let top = screen.scroll_top;
            let bottom = screen.scroll_bottom;
            let removed = screen.rows.remove(top);
            screen.rows.insert(bottom, blank_row(columns));
            (removed, top == 0 && bottom + 1 == screen.rows.len())
        };
        if active == ActiveScreen::Primary && is_full_screen && self.scrollback_limit > 0 {
            if self.scrollback.len() == self.scrollback_limit {
                self.scrollback.pop_front();
            }
            self.scrollback.push_back(removed);
        }
    }

    fn active_ref(&self) -> &Screen {
        match self.active {
            ActiveScreen::Primary => &self.primary,
            ActiveScreen::Alternate => &self.alternate,
        }
    }

    fn active_mut(&mut self) -> &mut Screen {
        match self.active {
            ActiveScreen::Primary => &mut self.primary,
            ActiveScreen::Alternate => &mut self.alternate,
        }
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn blank_rows(size: GridSize) -> Vec<Row> {
    (0..size.rows())
        .map(|_| blank_row(size.columns()))
        .collect()
}

fn blank_row(columns: usize) -> Row {
    vec![Cell::default(); columns]
}

fn resize_screen(screen: &mut Screen, size: GridSize) {
    let old_columns = screen.rows.first().map_or(size.columns(), Vec::len);
    if old_columns == size.columns() {
        screen
            .rows
            .resize_with(size.rows(), || blank_row(size.columns()));
        for row in &mut screen.rows {
            resize_row(row, size.columns());
        }
    } else {
        screen.rows = reflow_rows(&screen.rows, size);
    }
    screen.cursor.row = screen.cursor.row.min(size.rows() - 1);
    screen.cursor.column = screen.cursor.column.min(size.columns() - 1);
    screen.wrap_pending = false;
    screen.scroll_top = 0;
    screen.scroll_bottom = size.rows() - 1;
}

fn reflow_rows(source: &[Row], size: GridSize) -> Vec<Row> {
    let mut destination = blank_rows(size);
    let mut destination_row = 0;
    let mut destination_column = 0;

    for source_row in source {
        let content_end = source_row
            .iter()
            .rposition(|cell| !cell.is_semantically_blank())
            .map_or(0, |index| index + 1);

        for cell in source_row[..content_end]
            .iter()
            .filter(|cell| cell.width() != CellWidth::Continuation)
        {
            let cell_width = match cell.width() {
                CellWidth::Wide if size.columns() > 1 => 2,
                CellWidth::Wide | CellWidth::Single => 1,
                CellWidth::Continuation => unreachable!("continuations were filtered"),
            };
            if destination_column + cell_width > size.columns() {
                destination_row += 1;
                destination_column = 0;
            }
            if destination_row >= size.rows() {
                return destination;
            }

            if cell.width() == CellWidth::Wide && size.columns() == 1 {
                destination[destination_row][destination_column]
                    .set_text("�", CellWidth::Single)
                    .expect("replacement character fits in one cell");
            } else {
                destination[destination_row][destination_column] = cell.clone();
                if cell_width == 2 {
                    destination[destination_row][destination_column + 1]
                        .set_text("", CellWidth::Continuation)
                        .expect("empty continuation cell is valid");
                }
            }
            destination_column += cell_width;
        }

        destination_row += 1;
        destination_column = 0;
        if destination_row >= size.rows() {
            break;
        }
    }

    destination
}

fn resize_row(row: &mut Row, columns: usize) {
    row.resize(columns, Cell::default());
    row.truncate(columns);
    repair_row(row);
}

fn repair_row(row: &mut Row) {
    for column in 0..row.len() {
        match row[column].width() {
            CellWidth::Wide => {
                if row.get(column + 1).map(Cell::width) != Some(CellWidth::Continuation) {
                    row[column].reset();
                }
            }
            CellWidth::Continuation => {
                if column == 0 || row[column - 1].width() != CellWidth::Wide {
                    row[column].reset();
                }
            }
            CellWidth::Single => {}
        }
    }
}

fn erase_range(row: &mut Row, start: usize, end: usize) {
    for cell in &mut row[start..end] {
        cell.reset();
    }
    repair_row(row);
}
