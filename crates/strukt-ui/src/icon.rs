use std::time::Duration;

use iced::mouse;
use iced::widget::{canvas, canvas::Path};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Renderer, Theme, Vector};

use crate::{UiTheme, semantic_color};

pub const ACTIVITY_ICON_TRANSITION_MS: u16 = 180;
const ICON_VIEWBOX_SIZE: f32 = 20.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Files,
    Search,
    SourceControl,
    Sessions,
    Tasks,
    Connect,
    Extensions,
    Settings,
    Close,
    Promote,
    Demote,
    More,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IconCommand {
    Move(f32, f32),
    Line(f32, f32),
    Circle(f32, f32, f32),
    Rectangle(f32, f32, f32, f32),
    Close,
}

impl Icon {
    pub const ALL: [Self; 14] = [
        Self::Files,
        Self::Search,
        Self::SourceControl,
        Self::Sessions,
        Self::Tasks,
        Self::Connect,
        Self::Extensions,
        Self::Settings,
        Self::Close,
        Self::Promote,
        Self::Demote,
        Self::More,
        Self::Warning,
        Self::Error,
    ];

    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "the exhaustive vector geometry table keeps every icon auditable in one place"
    )]
    pub const fn geometry(self) -> &'static [IconCommand] {
        match self {
            Self::Files => &[
                IconCommand::Move(2.5, 5.5),
                IconCommand::Line(7.5, 5.5),
                IconCommand::Line(9.3, 7.5),
                IconCommand::Line(17.5, 7.5),
                IconCommand::Line(17.5, 16.5),
                IconCommand::Line(2.5, 16.5),
                IconCommand::Close,
            ],
            Self::Search => &[
                IconCommand::Circle(8.5, 8.5, 4.5),
                IconCommand::Move(12.0, 12.0),
                IconCommand::Line(16.0, 16.0),
            ],
            Self::SourceControl => &[
                IconCommand::Circle(6.0, 4.0, 1.7),
                IconCommand::Circle(14.0, 6.0, 1.7),
                IconCommand::Circle(6.0, 16.0, 1.7),
                IconCommand::Move(6.0, 5.7),
                IconCommand::Line(6.0, 14.3),
                IconCommand::Move(14.0, 7.7),
                IconCommand::Line(14.0, 8.8),
                IconCommand::Line(12.5, 11.2),
                IconCommand::Line(10.0, 12.8),
                IconCommand::Line(6.0, 12.8),
            ],
            Self::Sessions => &[
                IconCommand::Rectangle(3.0, 3.0, 14.0, 14.0),
                IconCommand::Move(3.0, 7.0),
                IconCommand::Line(17.0, 7.0),
                IconCommand::Move(8.0, 7.0),
                IconCommand::Line(8.0, 17.0),
            ],
            Self::Tasks => &[
                IconCommand::Move(4.0, 10.0),
                IconCommand::Line(7.5, 13.5),
                IconCommand::Line(16.0, 5.0),
            ],
            Self::Connect => &[
                IconCommand::Move(4.0, 16.0),
                IconCommand::Line(16.0, 4.0),
                IconCommand::Move(8.0, 4.0),
                IconCommand::Line(16.0, 4.0),
                IconCommand::Line(16.0, 12.0),
            ],
            Self::Extensions => &[
                IconCommand::Move(10.0, 2.5),
                IconCommand::Line(17.5, 10.0),
                IconCommand::Line(10.0, 17.5),
                IconCommand::Line(2.5, 10.0),
                IconCommand::Close,
                IconCommand::Move(7.5, 10.0),
                IconCommand::Line(12.5, 10.0),
            ],
            Self::Settings => &[
                IconCommand::Circle(10.0, 10.0, 2.5),
                IconCommand::Move(10.0, 2.5),
                IconCommand::Line(10.0, 4.5),
                IconCommand::Move(10.0, 15.5),
                IconCommand::Line(10.0, 17.5),
                IconCommand::Move(2.5, 10.0),
                IconCommand::Line(4.5, 10.0),
                IconCommand::Move(15.5, 10.0),
                IconCommand::Line(17.5, 10.0),
                IconCommand::Move(4.7, 4.7),
                IconCommand::Line(6.1, 6.1),
                IconCommand::Move(13.9, 13.9),
                IconCommand::Line(15.3, 15.3),
                IconCommand::Move(15.3, 4.7),
                IconCommand::Line(13.9, 6.1),
                IconCommand::Move(6.1, 13.9),
                IconCommand::Line(4.7, 15.3),
            ],
            Self::Close => &[
                IconCommand::Move(5.0, 5.0),
                IconCommand::Line(15.0, 15.0),
                IconCommand::Move(15.0, 5.0),
                IconCommand::Line(5.0, 15.0),
            ],
            Self::Promote => &[
                IconCommand::Move(10.0, 17.0),
                IconCommand::Line(10.0, 4.0),
                IconCommand::Move(5.0, 9.0),
                IconCommand::Line(10.0, 4.0),
                IconCommand::Line(15.0, 9.0),
            ],
            Self::Demote => &[
                IconCommand::Move(10.0, 3.0),
                IconCommand::Line(10.0, 16.0),
                IconCommand::Move(5.0, 11.0),
                IconCommand::Line(10.0, 16.0),
                IconCommand::Line(15.0, 11.0),
            ],
            Self::More => &[
                IconCommand::Circle(5.0, 10.0, 1.0),
                IconCommand::Circle(10.0, 10.0, 1.0),
                IconCommand::Circle(15.0, 10.0, 1.0),
            ],
            Self::Warning => &[
                IconCommand::Move(10.0, 2.5),
                IconCommand::Line(18.0, 17.0),
                IconCommand::Line(2.0, 17.0),
                IconCommand::Close,
                IconCommand::Move(10.0, 7.0),
                IconCommand::Line(10.0, 12.0),
                IconCommand::Circle(10.0, 14.5, 0.6),
            ],
            Self::Error => &[
                IconCommand::Circle(10.0, 10.0, 7.5),
                IconCommand::Move(10.0, 5.5),
                IconCommand::Line(10.0, 11.5),
                IconCommand::Circle(10.0, 14.0, 0.6),
            ],
        }
    }
}

#[derive(Debug)]
struct IconCanvas {
    icon: Icon,
    selected: bool,
    reduced_motion: bool,
    selection_surface: bool,
    resting: Color,
    active: Color,
    selection: Color,
    depth_back: Color,
    depth_mid: Color,
    highlight: Color,
}

#[derive(Debug, Default)]
struct IconCanvasState {
    initialized: bool,
    target: bool,
    progress: f32,
    last_redraw: Option<iced::time::Instant>,
}

impl<Message> canvas::Program<Message> for IconCanvas {
    type State = IconCanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };

        if !state.initialized {
            state.initialized = true;
            state.target = self.selected;
            state.progress = f32::from(self.selected);
            state.last_redraw = Some(*now);
            return None;
        }

        if state.target != self.selected {
            state.target = self.selected;
            state.last_redraw = Some(*now);
            if self.reduced_motion {
                state.progress = f32::from(self.selected);
                return None;
            }
        }

        let target = f32::from(state.target);
        if (state.progress - target).abs() <= f32::EPSILON {
            return None;
        }

        let elapsed = state
            .last_redraw
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last));
        state.last_redraw = Some(*now);
        let step = elapsed.as_secs_f32() / (f32::from(ACTIVITY_ICON_TRANSITION_MS) / 1_000.0);
        if state.target {
            state.progress = (state.progress + step).min(1.0);
        } else {
            state.progress = (state.progress - step).max(0.0);
        }

        ((state.progress - target).abs() > f32::EPSILON).then(canvas::Action::request_redraw)
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let progress = if state.initialized {
            state.progress
        } else {
            f32::from(self.selected)
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if self.selection_surface && progress > 0.0 {
            frame.fill_rectangle(
                Point::ORIGIN,
                bounds.size(),
                with_alpha(self.selection, self.selection.a * progress),
            );
        }

        let path = icon_path(self.icon);
        let scale = (bounds.width.min(bounds.height) / ICON_VIEWBOX_SIZE) * 0.48;
        let face = mix(self.resting, self.active, progress);
        let depth = 2.2 * progress;
        draw_pass(
            &mut frame,
            &path,
            bounds,
            scale,
            progress,
            Vector::new(0.0, depth),
            with_alpha(self.depth_back, self.depth_back.a * progress),
            2.6,
        );
        draw_pass(
            &mut frame,
            &path,
            bounds,
            scale,
            progress,
            Vector::new(0.0, depth * 0.55),
            with_alpha(self.depth_mid, self.depth_mid.a * progress),
            2.15,
        );
        draw_pass(
            &mut frame,
            &path,
            bounds,
            scale,
            progress,
            Vector::new(0.0, -0.4 * progress),
            face,
            1.45 + 0.45 * progress,
        );
        draw_pass(
            &mut frame,
            &path,
            bounds,
            scale,
            progress,
            Vector::new(-0.55 * progress, -0.9 * progress),
            with_alpha(self.highlight, self.highlight.a * progress * 0.58),
            0.7,
        );
        vec![frame.into_geometry()]
    }
}

#[must_use]
pub fn icon_view<'a, Message: 'a>(
    icon: Icon,
    selected: bool,
    reduced_motion: bool,
    selection_surface: bool,
    extent: f32,
    theme: &UiTheme,
) -> Element<'a, Message> {
    let tokens = theme.tokens;
    canvas(IconCanvas {
        icon,
        selected,
        reduced_motion,
        selection_surface,
        resting: semantic_color(tokens.text_muted),
        active: semantic_color(tokens.text_primary),
        selection: semantic_color(tokens.panel_active),
        depth_back: mix(
            semantic_color(tokens.canvas),
            semantic_color(tokens.text_primary),
            0.28,
        ),
        depth_mid: mix(
            semantic_color(tokens.canvas),
            semantic_color(tokens.text_primary),
            0.55,
        ),
        highlight: semantic_color(tokens.accent),
    })
    .width(Length::Fixed(extent))
    .height(Length::Fixed(extent))
    .into()
}

#[must_use]
pub const fn activity_transition_duration_ms(reduced_motion: bool) -> u16 {
    if reduced_motion {
        0
    } else {
        ACTIVITY_ICON_TRANSITION_MS
    }
}

fn icon_path(icon: Icon) -> Path {
    Path::new(|builder| {
        for command in icon.geometry() {
            match *command {
                IconCommand::Move(x, y) => builder.move_to(Point::new(x, y)),
                IconCommand::Line(x, y) => builder.line_to(Point::new(x, y)),
                IconCommand::Circle(x, y, radius) => builder.circle(Point::new(x, y), radius),
                IconCommand::Rectangle(x, y, width, height) => {
                    builder.rectangle(Point::new(x, y), iced::Size::new(width, height));
                }
                IconCommand::Close => builder.close(),
            }
        }
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "each explicit projection parameter documents one visual layer of the 2.5D icon"
)]
fn draw_pass(
    frame: &mut canvas::Frame,
    path: &Path,
    bounds: Rectangle,
    scale: f32,
    progress: f32,
    offset: Vector,
    color: Color,
    width: f32,
) {
    if color.a <= f32::EPSILON {
        return;
    }
    frame.with_save(|frame| {
        frame.translate(Vector::new(bounds.width / 2.0, bounds.height / 2.0));
        frame.rotate(Radians(-0.14 * progress));
        frame.scale_nonuniform(Vector::new(
            scale * (1.0 + 0.06 * progress),
            scale * (1.0 - 0.24 * progress),
        ));
        frame.translate(Vector::new(
            -ICON_VIEWBOX_SIZE / 2.0 + offset.x,
            -ICON_VIEWBOX_SIZE / 2.0 + offset.y,
        ));
        frame.stroke(
            path,
            canvas::Stroke::default()
                .with_color(color)
                .with_width(width)
                .with_line_cap(canvas::LineCap::Round)
                .with_line_join(canvas::LineJoin::Round),
        );
    });
}

fn mix(from: Color, to: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color {
        r: from.r + (to.r - from.r) * amount,
        g: from.g + (to.g - from.g) * amount,
        b: from.b + (to.b - from.b) * amount,
        a: from.a + (to.a - from.a) * amount,
    }
}

const fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}
