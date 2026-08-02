use std::collections::HashMap;

use iced::Renderer;
use iced::advanced::text::editor::{Cursor, Edit, Position};
use iced::widget::text_editor::{Action, Content};
use strukt_editor::{
    CharRange, DocumentId, EditKind, EditTransaction, EditorWorkspace, EditorWorkspaceError,
    Replacement,
};
use strukt_language::{
    LspPosition, PositionEncoding, ScalarPosition, from_lsp_position, to_lsp_position,
};

#[derive(Default)]
pub(crate) struct EditorSurfaces {
    contents: HashMap<DocumentId, Content<Renderer>>,
}

impl EditorSurfaces {
    pub(crate) fn insert(&mut self, id: DocumentId, text: &str) {
        self.contents.insert(id, Content::with_text(text));
    }

    pub(crate) fn remove(&mut self, id: DocumentId) {
        self.contents.remove(&id);
    }

    pub(crate) fn content(&self, id: DocumentId) -> Option<&Content<Renderer>> {
        self.contents.get(&id)
    }

    pub(crate) fn cursor_offsets(
        &self,
        id: DocumentId,
    ) -> Result<(usize, usize), EditorSurfaceError> {
        let content = self
            .content(id)
            .ok_or(EditorSurfaceError::MissingSurface(id))?;
        let cursor = content.cursor();
        let caret = offset_for_position(content, cursor.position)?;
        let anchor = cursor
            .selection
            .map_or(Ok(caret), |position| offset_for_position(content, position))?;
        Ok((caret, anchor))
    }

    pub(crate) fn rebuild(
        &mut self,
        workspace: &EditorWorkspace,
        id: DocumentId,
    ) -> Result<(), EditorWorkspaceError> {
        let document = workspace
            .document(id)
            .ok_or(EditorWorkspaceError::DocumentNotFound(id))?;
        self.insert(id, &document.text());
        Ok(())
    }

    pub(crate) fn restore_view(
        &mut self,
        id: DocumentId,
        cursor: usize,
        selection_anchor: usize,
        scroll_line: f32,
    ) -> Result<(), EditorSurfaceError> {
        let content = self.content_mut(id)?;
        let position = position_for_offset(content, cursor)?;
        let selection = (selection_anchor != cursor)
            .then(|| position_for_offset(content, selection_anchor))
            .transpose()?;
        content.move_to(Cursor {
            position,
            selection,
        });
        let scroll_lines = scroll_line.round();
        if scroll_lines != 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss,
                reason = "persisted editor scroll is bounded by Iced's i32 action contract"
            )]
            content.perform(Action::Scroll {
                lines: scroll_lines.clamp(i32::MIN as f32, i32::MAX as f32) as i32,
            });
        }
        Ok(())
    }

    pub(crate) fn move_to_lsp(
        &mut self,
        id: DocumentId,
        position: LspPosition,
        encoding: PositionEncoding,
    ) -> Result<(), EditorSurfaceError> {
        let content = self.content_mut(id)?;
        let text = content.text();
        let scalar = from_lsp_position(&text, position, encoding)
            .map_err(|_| EditorSurfaceError::InvalidCursor)?;
        let line_index =
            usize::try_from(scalar.line).map_err(|_| EditorSurfaceError::InvalidCursor)?;
        let scalar_column =
            usize::try_from(scalar.character).map_err(|_| EditorSurfaceError::InvalidCursor)?;
        let line = content
            .line(line_index)
            .ok_or(EditorSurfaceError::InvalidCursor)?;
        let column = if scalar_column == line.text.chars().count() {
            line.text.len()
        } else {
            line.text
                .char_indices()
                .nth(scalar_column)
                .map(|(index, _)| index)
                .ok_or(EditorSurfaceError::InvalidCursor)?
        };
        content.move_to(Cursor {
            position: Position {
                line: line_index,
                column,
            },
            selection: None,
        });
        Ok(())
    }

    pub(crate) fn current_lsp_position(
        &self,
        id: DocumentId,
        encoding: PositionEncoding,
    ) -> Result<LspPosition, EditorSurfaceError> {
        let content = self
            .content(id)
            .ok_or(EditorSurfaceError::MissingSurface(id))?;
        let position = content.cursor().position;
        let line = content
            .line(position.line)
            .ok_or(EditorSurfaceError::InvalidCursor)?;
        if position.column > line.text.len() || !line.text.is_char_boundary(position.column) {
            return Err(EditorSurfaceError::InvalidCursor);
        }
        let scalar_column = line.text[..position.column].chars().count();
        let scalar = ScalarPosition::new(
            u32::try_from(position.line).map_err(|_| EditorSurfaceError::InvalidCursor)?,
            u32::try_from(scalar_column).map_err(|_| EditorSurfaceError::InvalidCursor)?,
        );
        to_lsp_position(&content.text(), scalar, encoding)
            .map_err(|_| EditorSurfaceError::InvalidCursor)
    }

    pub(crate) fn insert_completion(
        &mut self,
        workspace: &mut EditorWorkspace,
        id: DocumentId,
        expected_revision: strukt_editor::Revision,
        insertion: &str,
        language_range: Option<strukt_language::LanguageRange>,
        encoding: PositionEncoding,
    ) -> Result<(), EditorSurfaceError> {
        let (caret, anchor) = self.cursor_offsets(id)?;
        let range = if let Some(range) = language_range {
            let text = self
                .content(id)
                .ok_or(EditorSurfaceError::MissingSurface(id))?
                .text();
            CharRange::new(
                scalar_offset_for_lsp(&text, range.start, encoding)?,
                scalar_offset_for_lsp(&text, range.end, encoding)?,
            )?
        } else {
            CharRange::new(caret.min(anchor), caret.max(anchor))?
        };
        workspace.edit(
            id,
            EditTransaction::new(
                expected_revision,
                vec![Replacement::new(range, insertion.to_owned())],
            )?,
            EditKind::Other,
            caret,
            range.start + insertion.chars().count(),
        )?;
        self.rebuild(workspace, id)?;
        let cursor = range.start + insertion.chars().count();
        self.restore_view(id, cursor, cursor, 0.0)?;
        Ok(())
    }

    pub(crate) fn perform(
        &mut self,
        workspace: &mut EditorWorkspace,
        id: DocumentId,
        action: Action,
    ) -> Result<(), EditorSurfaceError> {
        if !action.is_edit() {
            self.content_mut(id)?.perform(action);
            return Ok(());
        }

        let (transaction, kind, cursor_before, cursor_after, rebuild) = {
            let content = self
                .content(id)
                .ok_or(EditorSurfaceError::MissingSurface(id))?;
            let document = workspace
                .document(id)
                .ok_or(EditorWorkspaceError::DocumentNotFound(id))?;
            transaction_for_action(content, document.revision(), &action)?
        };
        workspace.edit(id, transaction, kind, cursor_before, cursor_after)?;
        if rebuild {
            self.rebuild(workspace, id)?;
        } else {
            self.content_mut(id)?.perform(action);
        }
        Ok(())
    }

    fn content_mut(
        &mut self,
        id: DocumentId,
    ) -> Result<&mut Content<Renderer>, EditorSurfaceError> {
        self.contents
            .get_mut(&id)
            .ok_or(EditorSurfaceError::MissingSurface(id))
    }
}

fn scalar_offset_for_lsp(
    text: &str,
    position: LspPosition,
    encoding: PositionEncoding,
) -> Result<usize, EditorSurfaceError> {
    let scalar = from_lsp_position(text, position, encoding)
        .map_err(|_| EditorSurfaceError::InvalidCursor)?;
    let target_line =
        usize::try_from(scalar.line).map_err(|_| EditorSurfaceError::InvalidCursor)?;
    let mut offset = 0;
    for (line_index, line) in text.split_inclusive('\n').enumerate() {
        if line_index == target_line {
            return Ok(offset
                + usize::try_from(scalar.character)
                    .map_err(|_| EditorSurfaceError::InvalidCursor)?);
        }
        offset += line.chars().count();
    }
    if target_line == text.lines().count() && text.ends_with('\n') {
        return Ok(offset);
    }
    Err(EditorSurfaceError::InvalidCursor)
}

fn transaction_for_action(
    content: &Content<Renderer>,
    revision: strukt_editor::Revision,
    action: &Action,
) -> Result<(EditTransaction, EditKind, usize, usize, bool), EditorSurfaceError> {
    let cursor = content.cursor();
    let caret = offset_for_position(content, cursor.position)?;
    let anchor = cursor
        .selection
        .map_or(Ok(caret), |position| offset_for_position(content, position))?;
    let selection = CharRange::new(caret.min(anchor), caret.max(anchor))?;
    let Action::Edit(edit) = action else {
        return Err(EditorSurfaceError::UnsupportedAction);
    };
    let (range, text, kind, rebuild) = match edit {
        Edit::Insert(character) => (selection, character.to_string(), EditKind::Typing, false),
        Edit::Paste(text) => (selection, text.as_str().to_owned(), EditKind::Other, false),
        Edit::Enter => (
            selection,
            line_ending(content).to_owned(),
            EditKind::Typing,
            false,
        ),
        Edit::Backspace | Edit::Delete if selection.start != selection.end => {
            (selection, String::new(), EditKind::Typing, false)
        }
        Edit::Backspace => (
            CharRange::new(previous_offset(content, cursor.position, caret)?, caret)?,
            String::new(),
            EditKind::Typing,
            false,
        ),
        Edit::Delete => (
            CharRange::new(caret, next_offset(content, cursor.position, caret)?)?,
            String::new(),
            EditKind::Typing,
            false,
        ),
        Edit::Indent => (
            CharRange::new(
                line_start_offset(content, cursor.position.line)?,
                line_start_offset(content, cursor.position.line)?,
            )?,
            "    ".to_owned(),
            EditKind::Other,
            true,
        ),
        Edit::Unindent => {
            let start = line_start_offset(content, cursor.position.line)?;
            let removable = content.line(cursor.position.line).map_or(0, |line| {
                line.text
                    .chars()
                    .take(4)
                    .take_while(|character| *character == ' ')
                    .count()
            });
            (
                CharRange::new(start, start + removable)?,
                String::new(),
                EditKind::Other,
                true,
            )
        }
    };
    let cursor_after = range.start + text.chars().count();
    Ok((
        EditTransaction::new(revision, vec![Replacement::new(range, text)])?,
        kind,
        caret,
        cursor_after,
        rebuild,
    ))
}

fn offset_for_position(
    content: &Content<Renderer>,
    position: iced::advanced::text::editor::Position,
) -> Result<usize, EditorSurfaceError> {
    let start = line_start_offset(content, position.line)?;
    let line = content
        .line(position.line)
        .ok_or(EditorSurfaceError::InvalidCursor)?;
    if position.column > line.text.len() || !line.text.is_char_boundary(position.column) {
        return Err(EditorSurfaceError::InvalidCursor);
    }
    Ok(start + line.text[..position.column].chars().count())
}

fn position_for_offset(
    content: &Content<Renderer>,
    mut offset: usize,
) -> Result<Position, EditorSurfaceError> {
    for line_index in 0..content.line_count() {
        let line = content
            .line(line_index)
            .ok_or(EditorSurfaceError::InvalidCursor)?;
        let line_chars = line.text.chars().count();
        if offset <= line_chars {
            let column = line
                .text
                .char_indices()
                .map(|(index, _)| index)
                .nth(offset)
                .unwrap_or(line.text.len());
            return Ok(Position {
                line: line_index,
                column,
            });
        }
        let ending_chars = line.ending.as_str().chars().count();
        if offset < line_chars + ending_chars {
            return Err(EditorSurfaceError::InvalidCursor);
        }
        offset -= line_chars + ending_chars;
    }
    Err(EditorSurfaceError::InvalidCursor)
}

fn line_start_offset(
    content: &Content<Renderer>,
    line_index: usize,
) -> Result<usize, EditorSurfaceError> {
    let mut offset = 0;
    for index in 0..line_index {
        let line = content
            .line(index)
            .ok_or(EditorSurfaceError::InvalidCursor)?;
        offset += line.text.chars().count() + line.ending.as_str().chars().count();
    }
    Ok(offset)
}

fn next_offset(
    content: &Content<Renderer>,
    position: iced::advanced::text::editor::Position,
    caret: usize,
) -> Result<usize, EditorSurfaceError> {
    let line = content
        .line(position.line)
        .ok_or(EditorSurfaceError::InvalidCursor)?;
    if position.column < line.text.len() {
        Ok(caret + 1)
    } else if position.line + 1 < content.line_count() {
        Ok(caret + line.ending.as_str().chars().count())
    } else {
        Ok(caret)
    }
}

fn previous_offset(
    content: &Content<Renderer>,
    position: Position,
    caret: usize,
) -> Result<usize, EditorSurfaceError> {
    if position.column > 0 || position.line == 0 {
        return Ok(caret.saturating_sub(1));
    }
    let previous = content
        .line(position.line - 1)
        .ok_or(EditorSurfaceError::InvalidCursor)?;
    Ok(caret.saturating_sub(previous.ending.as_str().chars().count()))
}

fn line_ending(content: &Content<Renderer>) -> &'static str {
    content.line(0).map_or("\n", |line| match line.ending {
        iced::advanced::text::editor::LineEnding::CrLf => "\r\n",
        iced::advanced::text::editor::LineEnding::Cr => "\r",
        iced::advanced::text::editor::LineEnding::LfCr => "\n\r",
        iced::advanced::text::editor::LineEnding::Lf
        | iced::advanced::text::editor::LineEnding::None => "\n",
    })
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum EditorSurfaceError {
    #[error("document {0:?} has no native editor surface")]
    MissingSurface(DocumentId),
    #[error("native editor cursor is outside the document")]
    InvalidCursor,
    #[error("unsupported native editor action")]
    UnsupportedAction,
    #[error(transparent)]
    Workspace(#[from] EditorWorkspaceError),
    #[error(transparent)]
    Transaction(#[from] strukt_editor::TransactionError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use strukt_editor::{DiskRevision, OpenDisposition, RelativeDocumentPath};
    use strukt_language::{LspPosition, PositionEncoding};
    use strukt_workspace::WorkspaceRoot;

    #[test]
    fn surface_edits_apply_one_domain_transaction_and_pin_preview() {
        let directory = tempfile::tempdir().unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        let mut workspace = EditorWorkspace::new(root.id().clone());
        let id = workspace
            .open(
                RelativeDocumentPath::new("file.txt").unwrap(),
                "abc",
                DiskRevision::new("disk"),
                false,
                OpenDisposition::Preview,
            )
            .unwrap();
        let mut surfaces = EditorSurfaces::default();
        surfaces.insert(id, "abc");
        surfaces.content_mut(id).unwrap().move_to(Cursor {
            position: Position { line: 0, column: 3 },
            selection: None,
        });

        surfaces
            .perform(&mut workspace, id, Action::Edit(Edit::Insert('!')))
            .unwrap();

        assert_eq!(workspace.document(id).unwrap().text(), "abc!");
        assert_eq!(surfaces.content(id).unwrap().text(), "abc!");
        assert!(workspace.view_state().tabs[0].pinned);
    }

    #[test]
    fn cursor_offsets_round_trip_across_unicode_and_lines() {
        let directory = tempfile::tempdir().unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        let mut workspace = EditorWorkspace::new(root.id().clone());
        let id = workspace
            .open(
                RelativeDocumentPath::new("file.txt").unwrap(),
                "éx\n東京",
                DiskRevision::new("disk"),
                false,
                OpenDisposition::Pinned,
            )
            .unwrap();
        let mut surfaces = EditorSurfaces::default();
        surfaces.insert(id, "éx\n東京");
        surfaces.content_mut(id).unwrap().move_to(Cursor {
            position: Position { line: 1, column: 6 },
            selection: Some(Position { line: 0, column: 2 }),
        });

        assert_eq!(surfaces.cursor_offsets(id).unwrap(), (5, 1));
        surfaces.restore_view(id, 5, 1, 0.0).unwrap();
        assert_eq!(surfaces.cursor_offsets(id).unwrap(), (5, 1));
    }

    #[test]
    fn unicode_byte_columns_are_converted_to_domain_character_offsets() {
        let directory = tempfile::tempdir().unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        let mut workspace = EditorWorkspace::new(root.id().clone());
        let id = workspace
            .open(
                RelativeDocumentPath::new("file.txt").unwrap(),
                "éx",
                DiskRevision::new("disk"),
                false,
                OpenDisposition::Pinned,
            )
            .unwrap();
        let mut surfaces = EditorSurfaces::default();
        surfaces.insert(id, "éx");
        surfaces.content_mut(id).unwrap().move_to(Cursor {
            position: Position { line: 0, column: 2 },
            selection: None,
        });

        surfaces
            .perform(&mut workspace, id, Action::Edit(Edit::Insert('!')))
            .unwrap();

        assert_eq!(workspace.document(id).unwrap().text(), "é!x");
        assert_eq!(surfaces.cursor_offsets(id).unwrap(), (2, 2));
    }

    #[test]
    fn diagnostic_navigation_converts_utf16_positions_for_the_native_editor() {
        let directory = tempfile::tempdir().unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        let mut workspace = EditorWorkspace::new(root.id().clone());
        let id = workspace
            .open(
                RelativeDocumentPath::new("file.txt").unwrap(),
                "one\n😀value",
                DiskRevision::new("disk"),
                false,
                OpenDisposition::Pinned,
            )
            .unwrap();
        let mut surfaces = EditorSurfaces::default();
        surfaces.insert(id, "one\n😀value");

        surfaces
            .move_to_lsp(id, LspPosition::new(1, 2), PositionEncoding::Utf16)
            .unwrap();

        assert_eq!(surfaces.cursor_offsets(id).unwrap(), (5, 5));
    }

    #[test]
    fn completion_insertion_is_one_editor_transaction_and_one_undo_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        let mut workspace = EditorWorkspace::new(root.id().clone());
        let id = workspace
            .open(
                RelativeDocumentPath::new("file.rs").unwrap(),
                "pri",
                DiskRevision::new("disk"),
                false,
                OpenDisposition::Pinned,
            )
            .unwrap();
        let mut surfaces = EditorSurfaces::default();
        surfaces.insert(id, "pri");
        surfaces.restore_view(id, 3, 0, 0.0).unwrap();

        surfaces
            .insert_completion(
                &mut workspace,
                id,
                strukt_editor::Revision::INITIAL,
                "println!",
                None,
                PositionEncoding::Utf16,
            )
            .unwrap();
        assert_eq!(workspace.document(id).unwrap().text(), "println!");
        workspace.undo(id).unwrap();
        assert_eq!(workspace.document(id).unwrap().text(), "pri");
    }

    #[test]
    fn backspace_at_a_crlf_line_start_removes_the_complete_line_ending() {
        let directory = tempfile::tempdir().unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        let mut workspace = EditorWorkspace::new(root.id().clone());
        let id = workspace
            .open(
                RelativeDocumentPath::new("file.txt").unwrap(),
                "a\r\nb",
                DiskRevision::new("disk"),
                false,
                OpenDisposition::Pinned,
            )
            .unwrap();
        let mut surfaces = EditorSurfaces::default();
        surfaces.insert(id, "a\r\nb");
        surfaces.content_mut(id).unwrap().move_to(Cursor {
            position: Position { line: 1, column: 0 },
            selection: None,
        });

        surfaces
            .perform(&mut workspace, id, Action::Edit(Edit::Backspace))
            .unwrap();

        assert_eq!(workspace.document(id).unwrap().text(), "ab");
    }
}
