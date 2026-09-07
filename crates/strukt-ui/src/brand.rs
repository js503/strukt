use std::time::Duration;

use iced::mouse;
use iced::widget::{canvas, canvas::Path, tooltip};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, Vector};

use crate::{UiTheme, semantic_color};

pub const BRAND_MARK_EXTENT: f32 = 29.0;
pub const BRAND_IDENTITY_SLOT_WIDTH: f32 = 42.0;
pub const BRAND_MOTION_CYCLE_MS: u64 = 8_400;
const BRAND_VIEWBOX_SIZE: f32 = 32.0;
const ACTIVE_START_MS: u64 = 3_200;
const ACTIVE_END_MS: u64 = 4_800;
const FRAME_MS: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrandLine {
    pub from: (f32, f32),
    pub to: (f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrandCommand {
    Outer(BrandLine),
    Plane(BrandLine),
    Bolt(BrandLine),
    Node { x: f32, y: f32, radius: f32 },
}

const BRAND_GEOMETRY: [BrandCommand; 16] = [
    BrandCommand::Outer(line(16.0, 2.5, 27.5, 9.0)),
    BrandCommand::Outer(line(27.5, 9.0, 27.5, 22.5)),
    BrandCommand::Outer(line(27.5, 22.5, 16.0, 29.5)),
    BrandCommand::Outer(line(16.0, 29.5, 4.5, 22.5)),
    BrandCommand::Outer(line(4.5, 22.5, 4.5, 9.0)),
    BrandCommand::Outer(line(4.5, 9.0, 16.0, 2.5)),
    BrandCommand::Plane(line(4.5, 9.0, 16.0, 15.7)),
    BrandCommand::Plane(line(27.5, 9.0, 16.0, 15.7)),
    BrandCommand::Plane(line(16.0, 15.7, 16.0, 29.5)),
    BrandCommand::Bolt(line(25.0, 11.7, 17.9, 21.4)),
    BrandCommand::Bolt(line(17.9, 21.4, 22.0, 19.8)),
    BrandCommand::Bolt(line(22.0, 19.8, 18.6, 27.2)),
    BrandCommand::Bolt(line(18.6, 27.2, 26.8, 15.8)),
    BrandCommand::Bolt(line(26.8, 15.8, 22.7, 17.4)),
    BrandCommand::Bolt(line(22.7, 17.4, 25.0, 11.7)),
    BrandCommand::Node {
        x: 22.0,
        y: 19.0,
        radius: 0.3,
    },
];

const BOLT_POINTS: [Point; 6] = [
    Point::new(25.0, 11.7),
    Point::new(17.9, 21.4),
    Point::new(22.0, 19.8),
    Point::new(18.6, 27.2),
    Point::new(26.8, 15.8),
    Point::new(22.7, 17.4),
];

const fn line(from_x: f32, from_y: f32, to_x: f32, to_y: f32) -> BrandLine {
    BrandLine {
        from: (from_x, from_y),
        to: (to_x, to_y),
    }
}

#[must_use]
pub const fn brand_geometry() -> &'static [BrandCommand] {
    &BRAND_GEOMETRY
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrandMotion {
    pub active: bool,
    pub current_progress: f32,
    pub node_scale: f32,
    pub node_opacity: f32,
    pub next_redraw_ms: Option<u64>,
}

#[must_use]
pub fn brand_motion_sample(elapsed_ms: u64, reduced_motion: bool, focused: bool) -> BrandMotion {
    if reduced_motion || !focused {
        return BrandMotion::resting(None);
    }

    let cycle_ms = elapsed_ms % BRAND_MOTION_CYCLE_MS;
    if cycle_ms < ACTIVE_START_MS {
        return BrandMotion::resting(Some(ACTIVE_START_MS - cycle_ms));
    }
    if cycle_ms >= ACTIVE_END_MS {
        return BrandMotion::resting(Some(BRAND_MOTION_CYCLE_MS - cycle_ms + ACTIVE_START_MS));
    }

    let linear = Duration::from_millis(cycle_ms - ACTIVE_START_MS).as_secs_f32()
        / Duration::from_millis(ACTIVE_END_MS - ACTIVE_START_MS).as_secs_f32();
    let eased = linear * linear * (3.0 - 2.0 * linear);
    let pulse = (std::f32::consts::PI * eased).sin();
    BrandMotion {
        active: true,
        current_progress: eased,
        node_scale: 1.0 + pulse * 0.08,
        node_opacity: 0.78 + pulse * 0.18,
        next_redraw_ms: Some(FRAME_MS),
    }
}

impl BrandMotion {
    const fn resting(next_redraw_ms: Option<u64>) -> Self {
        Self {
            active: false,
            current_progress: 0.0,
            node_scale: 1.0,
            node_opacity: 0.78,
            next_redraw_ms,
        }
    }
}

#[derive(Debug)]
struct BrandCanvas {
    reduced_motion: bool,
    structure: Color,
    planes: Color,
    current: Color,
    surface: Color,
}

#[derive(Debug)]
struct BrandCanvasState {
    cycle_started_at: Option<iced::time::Instant>,
    focused: bool,
    motion: BrandMotion,
}

impl Default for BrandCanvasState {
    fn default() -> Self {
        Self {
            cycle_started_at: None,
            focused: true,
            motion: BrandMotion::resting(None),
        }
    }
}

impl<Message> canvas::Program<Message> for BrandCanvas {
    type State = BrandCanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Window(iced::window::Event::Focused) => {
                state.focused = true;
                state.cycle_started_at = None;
                Some(canvas::Action::request_redraw())
            }
            canvas::Event::Window(iced::window::Event::Unfocused) => {
                state.focused = false;
                state.motion = BrandMotion::resting(None);
                None
            }
            canvas::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                let started = state.cycle_started_at.get_or_insert(*now);
                let elapsed = now.saturating_duration_since(*started);
                state.motion = brand_motion_sample(
                    elapsed.as_millis().try_into().unwrap_or(u64::MAX),
                    self.reduced_motion,
                    state.focused,
                );
                state.motion.next_redraw_ms.map(|delay| {
                    canvas::Action::request_redraw_at(*now + Duration::from_millis(delay))
                })
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let scale = bounds.width.min(bounds.height) / BRAND_VIEWBOX_SIZE;
        frame.with_save(|frame| {
            frame.translate(Vector::new(
                (bounds.width - BRAND_VIEWBOX_SIZE * scale) / 2.0,
                (bounds.height - BRAND_VIEWBOX_SIZE * scale) / 2.0,
            ));
            frame.scale(scale);
            let pulse = ((state.motion.node_opacity - 0.78) / 0.18).clamp(0.0, 1.0);
            for (points, shade) in [
                ([(16.0, 2.5), (27.5, 9.0), (16.0, 15.7), (4.5, 9.0)], 0.38),
                ([(4.5, 9.0), (16.0, 15.7), (16.0, 29.5), (4.5, 22.5)], 0.16),
                (
                    [(16.0, 15.7), (27.5, 9.0), (27.5, 22.5), (16.0, 29.5)],
                    0.045,
                ),
            ] {
                let points = points.map(|(x, y)| Point::new(x, y));
                frame.fill(
                    &polygon(&points),
                    mix(self.surface, self.current, shade + pulse * 0.025),
                );
            }
            draw_brand_layer(frame, BrandLayer::Outer, self.structure, 1.15);
            draw_brand_layer(frame, BrandLayer::Plane, self.planes, 0.85);
            frame.fill(
                &polygon(&[
                    Point::new(4.5, 9.0),
                    Point::new(16.0, 2.5),
                    Point::new(15.4, 3.8),
                    Point::new(5.5, 9.6),
                ]),
                self.structure,
            );
            let bolt = polygon(&BOLT_POINTS);
            frame.stroke(
                &bolt,
                canvas::Stroke::default()
                    .with_color(self.surface)
                    .with_width(0.65),
            );
            frame.fill(&bolt, mix(self.surface, self.current, 0.7 + pulse * 0.2));
            frame.stroke(
                &Path::new(|p| {
                    p.move_to(BOLT_POINTS[1]);
                    p.line_to(BOLT_POINTS[2]);
                    p.line_to(BOLT_POINTS[3]);
                }),
                canvas::Stroke::default()
                    .with_color(mix(self.current, self.structure, 0.45))
                    .with_width(0.35),
            );
            draw_node(frame, self.current, state.motion);
        });
        vec![frame.into_geometry()]
    }
}

#[must_use]
pub fn brand_mark<'a, Message: 'a>(reduced_motion: bool, theme: &UiTheme) -> Element<'a, Message> {
    let tokens = theme.tokens;
    let mark = canvas(BrandCanvas {
        reduced_motion,
        structure: semantic_color(tokens.text_primary),
        planes: semantic_color(tokens.text_muted),
        current: semantic_color(tokens.accent),
        surface: semantic_color(tokens.canvas),
    })
    .width(Length::Fixed(BRAND_MARK_EXTENT))
    .height(Length::Fixed(BRAND_MARK_EXTENT));

    tooltip(mark, "strukt", tooltip::Position::Bottom).into()
}

#[derive(Clone, Copy)]
enum BrandLayer {
    Outer,
    Plane,
}

fn draw_brand_layer(frame: &mut canvas::Frame, layer: BrandLayer, color: Color, width: f32) {
    let path = Path::new(|builder| {
        for command in brand_geometry() {
            let candidate = match (*command, layer) {
                (BrandCommand::Outer(line), BrandLayer::Outer)
                | (BrandCommand::Plane(line), BrandLayer::Plane) => Some(line),
                _ => None,
            };
            if let Some(line) = candidate {
                builder.move_to(Point::new(line.from.0, line.from.1));
                builder.line_to(Point::new(line.to.0, line.to.1));
            }
        }
    });
    frame.stroke(
        &path,
        canvas::Stroke::default()
            .with_color(color)
            .with_width(width)
            .with_line_cap(canvas::LineCap::Butt)
            .with_line_join(canvas::LineJoin::Miter),
    );
}

fn draw_node(frame: &mut canvas::Frame, color: Color, motion: BrandMotion) {
    let (x, y, radius) = brand_geometry()
        .iter()
        .find_map(|command| match command {
            BrandCommand::Node { x, y, radius } => Some((*x, *y, *radius)),
            _ => None,
        })
        .expect("brand geometry defines its energy node");
    frame.fill(
        &Path::circle(Point::new(x, y), radius * motion.node_scale),
        Color {
            a: color.a * motion.node_opacity,
            ..color
        },
    );
}

fn polygon(points: &[Point]) -> Path {
    Path::new(|builder| {
        builder.move_to(points[0]);
        for point in &points[1..] {
            builder.line_to(*point);
        }
        builder.close();
    })
}

fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * amount,
        g: a.g + (b.g - a.g) * amount,
        b: a.b + (b.b - a.b) * amount,
        a: 1.0,
    }
}
