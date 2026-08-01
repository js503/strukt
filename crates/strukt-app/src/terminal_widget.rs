use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Font, Length, Pixels, Point, Rectangle, Renderer, Size, Theme};
use strukt_terminal::{
    CellAttributes, CellWidth, Color as TerminalColor, Selection, TerminalCoordinate,
    TerminalPaneId, TerminalSize, TerminalSnapshot,
};
use strukt_theme::{Rgb, ThemeTokens};

const CELL_WIDTH: f32 = 8.4;
const CELL_HEIGHT: f32 = 17.0;
const FONT_SIZE: f32 = 13.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TerminalWidgetEvent {
    Focus(TerminalPaneId),
    Resize {
        pane: TerminalPaneId,
        size: TerminalSize,
    },
    Select {
        pane: TerminalPaneId,
        start: TerminalCoordinate,
        end: TerminalCoordinate,
    },
    Scroll {
        pane: TerminalPaneId,
        lines: i32,
    },
}

#[derive(Default)]
pub(crate) struct TerminalWidgetState {
    drag_anchor: Option<TerminalCoordinate>,
    last_size: Option<(u16, u16)>,
}

pub(crate) struct TerminalWidget {
    pane: TerminalPaneId,
    snapshot: TerminalSnapshot,
    tokens: ThemeTokens,
    focused: bool,
    selection: Option<Selection>,
}

impl TerminalWidget {
    pub(crate) const fn new(
        pane: TerminalPaneId,
        snapshot: TerminalSnapshot,
        tokens: ThemeTokens,
        focused: bool,
        selection: Option<Selection>,
    ) -> Self {
        Self {
            pane,
            snapshot,
            tokens,
            focused,
            selection,
        }
    }

    pub(crate) fn view(self) -> iced::Element<'static, TerminalWidgetEvent> {
        canvas(self).width(Length::Fill).height(Length::Fill).into()
    }
}

impl canvas::Program<TerminalWidgetEvent> for TerminalWidget {
    type State = TerminalWidgetState;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "bounded pointer deltas intentionally become whole terminal scroll lines"
    )]
    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<TerminalWidgetEvent>> {
        let size = terminal_size(bounds);
        let dimensions = (size.rows(), size.columns());
        if state.last_size != Some(dimensions) {
            state.last_size = Some(dimensions);
            return Some(canvas::Action::publish(TerminalWidgetEvent::Resize {
                pane: self.pane,
                size,
            }));
        }

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let coordinate = coordinate_at(bounds, cursor)?;
                state.drag_anchor = Some(coordinate);
                Some(canvas::Action::publish(TerminalWidgetEvent::Focus(self.pane)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let start = state.drag_anchor?;
                let end = coordinate_at(bounds, cursor)?;
                Some(
                    canvas::Action::publish(TerminalWidgetEvent::Select {
                        pane: self.pane,
                        start,
                        end,
                    })
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag_anchor = None;
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y.round() as i32,
                    mouse::ScrollDelta::Pixels { y, .. } => (y / CELL_HEIGHT).round() as i32,
                };
                (lines != 0).then(|| {
                    canvas::Action::publish(TerminalWidgetEvent::Scroll {
                        pane: self.pane,
                        lines,
                    })
                    .and_capture()
                })
            }
            _ => None,
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "bounded visible grid coordinates are converted to renderer pixels"
    )]
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            iced_color(self.tokens.terminal_background),
        );

        for (row_index, row) in self.snapshot.rows().iter().enumerate() {
            let y = row_index as f32 * CELL_HEIGHT;
            if y >= bounds.height {
                break;
            }
            for (column_index, cell) in row.iter().enumerate() {
                if cell.width() == CellWidth::Continuation {
                    continue;
                }
                let x = column_index as f32 * CELL_WIDTH;
                if x >= bounds.width {
                    break;
                }
                let (foreground, background) = resolved_colors(
                    cell.foreground,
                    cell.background,
                    cell.attributes,
                    self.tokens,
                );
                let coordinate = TerminalCoordinate {
                    row: row_index,
                    column: column_index,
                };
                let selected = self.selection.is_some_and(|selection| {
                    selection.start() <= coordinate && coordinate <= selection.end()
                });
                let background = if selected {
                    iced_color(self.tokens.terminal_selection)
                } else {
                    background
                };
                if background != iced_color(self.tokens.terminal_background) {
                    let columns = usize::from(cell.width() == CellWidth::Wide) + 1;
                    frame.fill_rectangle(
                        Point::new(x, y),
                        Size::new(CELL_WIDTH * columns as f32, CELL_HEIGHT),
                        background,
                    );
                }
                if cell.text() != " " {
                    frame.fill_text(canvas::Text {
                        content: cell.text().to_owned(),
                        position: Point::new(x, y + 1.0),
                        color: foreground,
                        size: Pixels(FONT_SIZE),
                        font: Font::MONOSPACE,
                        ..canvas::Text::default()
                    });
                }
                if cell.attributes.underline {
                    frame.fill_rectangle(
                        Point::new(x, y + CELL_HEIGHT - 2.0),
                        Size::new(CELL_WIDTH, 1.0),
                        foreground,
                    );
                }
            }
        }

        let cursor = self.snapshot.cursor();
        if self.focused && cursor.visible {
            frame.fill_rectangle(
                Point::new(
                    cursor.column as f32 * CELL_WIDTH,
                    cursor.row as f32 * CELL_HEIGHT,
                ),
                Size::new(2.0, CELL_HEIGHT),
                iced_color(self.tokens.terminal_cursor),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "nonnegative widget pixels are clamped into PTY character dimensions"
)]
fn terminal_size(bounds: Rectangle) -> TerminalSize {
    let rows = ((bounds.height / CELL_HEIGHT).floor() as u16).max(1);
    let columns = ((bounds.width / CELL_WIDTH).floor() as u16).max(1);
    TerminalSize::new(rows, columns).expect("clamped terminal widget size is nonempty")
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a cursor inside nonnegative widget bounds maps to a whole cell coordinate"
)]
fn coordinate_at(bounds: Rectangle, cursor: mouse::Cursor) -> Option<TerminalCoordinate> {
    let position = cursor.position_in(bounds)?;
    Some(TerminalCoordinate {
        row: (position.y / CELL_HEIGHT).floor() as usize,
        column: (position.x / CELL_WIDTH).floor() as usize,
    })
}

fn resolved_colors(
    foreground: TerminalColor,
    background: TerminalColor,
    attributes: CellAttributes,
    tokens: ThemeTokens,
) -> (Color, Color) {
    let foreground = terminal_color(foreground, tokens.terminal_foreground, tokens);
    let background = terminal_color(background, tokens.terminal_background, tokens);
    if attributes.inverse {
        (background, foreground)
    } else {
        (foreground, background)
    }
}

fn terminal_color(value: TerminalColor, default: Rgb, tokens: ThemeTokens) -> Color {
    match value {
        TerminalColor::Default => iced_color(default),
        TerminalColor::Indexed(index) => iced_color(indexed_color(index, tokens)),
        TerminalColor::Rgb(red, green, blue) => Color::from_rgb8(red, green, blue),
    }
}

fn indexed_color(index: u8, tokens: ThemeTokens) -> Rgb {
    match index {
        0..=15 => tokens.terminal_ansi[usize::from(index)],
        16..=231 => {
            let index = index - 16;
            let red = index / 36;
            let green = (index % 36) / 6;
            let blue = index % 6;
            Rgb::new(
                cube_component(red),
                cube_component(green),
                cube_component(blue),
            )
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            Rgb::new(gray, gray, gray)
        }
    }
}

const fn cube_component(component: u8) -> u8 {
    if component == 0 {
        0
    } else {
        55 + component * 40
    }
}

fn iced_color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

#[cfg(test)]
mod tests {
    use super::{indexed_color, terminal_size};
    use iced::Rectangle;
    use strukt_theme::{Rgb, ThemeMode, ThemeTokens};

    #[test]
    fn widget_bounds_map_to_a_nonempty_character_grid() {
        let size = terminal_size(Rectangle::new([0.0, 0.0].into(), [840.0, 340.0].into()));
        assert_eq!((size.rows(), size.columns()), (20, 100));

        let minimum = terminal_size(Rectangle::new([0.0, 0.0].into(), [0.0, 0.0].into()));
        assert_eq!((minimum.rows(), minimum.columns()), (1, 1));
    }

    #[test]
    fn indexed_palette_supports_ansi_cube_and_grayscale() {
        let tokens = ThemeTokens::builtin(ThemeMode::Dark);
        assert_eq!(indexed_color(0, tokens), tokens.terminal_ansi[0]);
        assert_eq!(indexed_color(16, tokens), Rgb::new(0, 0, 0));
        assert_eq!(indexed_color(231, tokens), Rgb::new(255, 255, 255));
        assert_eq!(indexed_color(232, tokens), Rgb::new(8, 8, 8));
        assert_eq!(indexed_color(255, tokens), Rgb::new(238, 238, 238));
    }
}
