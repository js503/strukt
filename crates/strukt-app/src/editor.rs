use std::collections::HashMap;

use iced::Renderer;
use iced::advanced::text::editor::{Edit, Motion};
use iced::widget::text_editor::{Action, Content};
use strukt_editor::{
    CharRange, DocumentId, EditKind, EditTransaction, EditorWorkspace, EditorWorkspaceError,
    Replacement,
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
        content.perform(Action::Move(Motion::DocumentStart));
        for _ in 0..cursor {
            content.perform(Action::Move(Motion::Right));
        }
        let distance = cursor.abs_diff(selection_anchor);
        let motion = if selection_anchor < cursor {
            Motion::Left
        } else {
            Motion::Right
        };
        for _ in 0..distance {
            content.perform(Action::Select(motion));
        }
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
            CharRange::new(caret.saturating_sub(1), caret)?,
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
    if position.column > line.text.chars().count() {
        return Err(EditorSurfaceError::InvalidCursor);
    }
    Ok(start + position.column)
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
    if position.column < line.text.chars().count() {
        Ok(caret + 1)
    } else if position.line + 1 < content.line_count() {
        Ok(caret + line.ending.as_str().chars().count())
    } else {
        Ok(caret)
    }
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
    use iced::advanced::text::editor::{Cursor, Position};
    use strukt_editor::{DiskRevision, OpenDisposition, RelativeDocumentPath};
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
}
