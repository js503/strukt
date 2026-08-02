use std::collections::BTreeMap;

use vte::{Params, Parser, Perform};

use crate::{
    CellAttributes, Color, EraseDisplay, EraseLine, Grid, GridSize, HyperlinkId, ResizeOutcome,
    TerminalModes, TerminalSnapshot,
};

const OSC_PAYLOAD_LIMIT: usize = 8 * 1024;
const OSC_PARSER_CAPACITY: usize = OSC_PAYLOAD_LIMIT + 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParserDiagnostics {
    pub discarded_sequences: u64,
    pub unsupported_sequences: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Pen {
    foreground: Color,
    background: Color,
    attributes: CellAttributes,
    hyperlink: Option<HyperlinkId>,
}

struct TerminalPerformer {
    grid: Grid,
    pen: Pen,
    title: Option<String>,
    diagnostics: ParserDiagnostics,
    modes: TerminalModes,
    hyperlink_targets: BTreeMap<HyperlinkId, String>,
    next_hyperlink_id: u32,
}

pub struct TerminalModel {
    parser: Parser<OSC_PARSER_CAPACITY>,
    performer: TerminalPerformer,
}

impl TerminalModel {
    #[must_use]
    pub fn new(size: GridSize, scrollback_limit: usize) -> Self {
        Self {
            parser: Parser::new_with_size(),
            performer: TerminalPerformer {
                grid: Grid::new(size, scrollback_limit),
                pen: Pen::default(),
                title: None,
                diagnostics: ParserDiagnostics::default(),
                modes: TerminalModes::default(),
                hyperlink_targets: BTreeMap::new(),
                next_hyperlink_id: 1,
            },
        }
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.performer, bytes);
    }

    #[must_use]
    pub fn snapshot(&self, viewport_offset: usize) -> TerminalSnapshot {
        let mut snapshot = self.performer.grid.snapshot(viewport_offset);
        snapshot.set_metadata(
            self.performer.title.clone(),
            self.performer.modes,
            self.performer.hyperlink_targets.clone(),
        );
        snapshot
    }

    #[must_use]
    pub const fn diagnostics(&self) -> ParserDiagnostics {
        self.performer.diagnostics
    }

    pub fn resize(&mut self, size: GridSize) -> ResizeOutcome {
        self.performer.grid.resize(size)
    }
}

impl Perform for TerminalPerformer {
    fn print(&mut self, character: char) {
        self.grid.print_styled(
            character,
            self.pen.foreground,
            self.pen.background,
            self.pen.attributes,
            self.pen.hyperlink,
        );
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => self.grid.print("\r"),
            b'\n' | 0x0b | 0x0c => self.grid.print("\n"),
            0x08 => self.grid.print("\u{8}"),
            b'\t' => self.grid.print("\t"),
            0x07 => {}
            _ => self.unsupported(),
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        let Some(command) = params.first() else {
            self.discarded();
            return;
        };
        let payload_len = params
            .iter()
            .skip(1)
            .map(|param| param.len())
            .sum::<usize>();
        if payload_len > OSC_PAYLOAD_LIMIT {
            self.discarded();
            return;
        }

        match (*command, params.get(1)) {
            (b"0" | b"2", Some(title)) => match std::str::from_utf8(title) {
                Ok(title) => self.title = Some(title.to_owned()),
                Err(_) => self.discarded(),
            },
            (b"8", _) => self.apply_hyperlink(params),
            _ => self.unsupported(),
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            self.discarded();
            return;
        }
        let values = flatten_params(params);
        match (action, intermediates) {
            ('m', []) => self.apply_sgr(&values),
            ('h', [b'?']) => self.set_private_modes(&values, true),
            ('l', [b'?']) => self.set_private_modes(&values, false),
            ('H' | 'f', []) => self.grid.set_cursor(
                usize::from(param(&values, 0, 1).saturating_sub(1)),
                usize::from(param(&values, 1, 1).saturating_sub(1)),
            ),
            ('A', []) => self.grid.move_cursor(
                -isize::try_from(param(&values, 0, 1)).unwrap_or(isize::MAX),
                0,
            ),
            ('B' | 'e', []) => self.grid.move_cursor(
                isize::try_from(param(&values, 0, 1)).unwrap_or(isize::MAX),
                0,
            ),
            ('C' | 'a', []) => self.grid.move_cursor(
                0,
                isize::try_from(param(&values, 0, 1)).unwrap_or(isize::MAX),
            ),
            ('D', []) => self.grid.move_cursor(
                0,
                -isize::try_from(param(&values, 0, 1)).unwrap_or(isize::MAX),
            ),
            ('G' | '`', []) => {
                let row = self.grid.cursor().row;
                self.grid
                    .set_cursor(row, usize::from(param(&values, 0, 1).saturating_sub(1)));
            }
            ('d', []) => {
                let column = self.grid.cursor().column;
                self.grid
                    .set_cursor(usize::from(param(&values, 0, 1).saturating_sub(1)), column);
            }
            ('J', []) => self.grid.erase_in_display(match param(&values, 0, 0) {
                0 => EraseDisplay::Below,
                1 => EraseDisplay::Above,
                _ => EraseDisplay::All,
            }),
            ('K', []) => self.grid.erase_in_line(match param(&values, 0, 0) {
                0 => EraseLine::Right,
                1 => EraseLine::Left,
                _ => EraseLine::All,
            }),
            ('@', []) => self
                .grid
                .insert_blank_characters(usize::from(param(&values, 0, 1))),
            ('P', []) => self
                .grid
                .delete_characters(usize::from(param(&values, 0, 1))),
            ('L', []) => self
                .grid
                .insert_blank_lines(usize::from(param(&values, 0, 1))),
            ('M', []) => self.grid.delete_lines(usize::from(param(&values, 0, 1))),
            ('S', []) => self.grid.scroll_up(usize::from(param(&values, 0, 1))),
            ('r', []) => {
                let bottom_default = u16::try_from(self.grid.size().rows()).unwrap_or(u16::MAX);
                let top = usize::from(param(&values, 0, 1).saturating_sub(1));
                let bottom = usize::from(
                    param(&values, 1, bottom_default)
                        .min(bottom_default)
                        .saturating_sub(1),
                );
                if self.grid.set_scroll_region(top, bottom).is_err() {
                    self.discarded();
                }
            }
            ('s', []) => self.grid.save_cursor(),
            ('u', []) => self.grid.restore_cursor(),
            _ => self.unsupported(),
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore || !intermediates.is_empty() {
            self.discarded();
            return;
        }
        match byte {
            b'7' => self.grid.save_cursor(),
            b'8' => self.grid.restore_cursor(),
            b'D' => self.grid.print("\n"),
            b'E' => self.grid.print("\r\n"),
            b'M' => self.grid.reverse_index(),
            _ => self.unsupported(),
        }
    }
}

impl TerminalPerformer {
    fn apply_sgr(&mut self, params: &[u16]) {
        let params = if params.is_empty() { &[0][..] } else { params };
        let mut index = 0;
        while index < params.len() {
            match params[index] {
                0 => self.pen = Pen::default(),
                1 => self.pen.attributes.bold = true,
                2 => self.pen.attributes.faint = true,
                3 => self.pen.attributes.italic = true,
                4 => self.pen.attributes.underline = true,
                7 => self.pen.attributes.inverse = true,
                9 => self.pen.attributes.strikethrough = true,
                22 => {
                    self.pen.attributes.bold = false;
                    self.pen.attributes.faint = false;
                }
                23 => self.pen.attributes.italic = false,
                24 => self.pen.attributes.underline = false,
                27 => self.pen.attributes.inverse = false,
                29 => self.pen.attributes.strikethrough = false,
                30..=37 => {
                    self.pen.foreground = Color::Indexed(
                        u8::try_from(params[index] - 30).expect("basic SGR color fits in u8"),
                    );
                }
                39 => self.pen.foreground = Color::Default,
                40..=47 => {
                    self.pen.background = Color::Indexed(
                        u8::try_from(params[index] - 40).expect("basic SGR color fits in u8"),
                    );
                }
                49 => self.pen.background = Color::Default,
                90..=97 => {
                    self.pen.foreground = Color::Indexed(
                        u8::try_from(params[index] - 90 + 8).expect("bright SGR color fits in u8"),
                    );
                }
                100..=107 => {
                    self.pen.background = Color::Indexed(
                        u8::try_from(params[index] - 100 + 8).expect("bright SGR color fits in u8"),
                    );
                }
                38 | 48 => {
                    let foreground = params[index] == 38;
                    if let Some((color, consumed)) = extended_color(&params[index + 1..]) {
                        if foreground {
                            self.pen.foreground = color;
                        } else {
                            self.pen.background = color;
                        }
                        index += consumed;
                    } else {
                        self.discarded();
                    }
                }
                _ => self.unsupported(),
            }
            index += 1;
        }
    }

    fn set_private_modes(&mut self, params: &[u16], enabled: bool) {
        for mode in params {
            match *mode {
                1049 if enabled => self.grid.enter_alternate_screen(),
                1049 => self.grid.leave_alternate_screen(),
                1 => self.modes.application_cursor_keys = enabled,
                25 => self.grid.set_cursor_visible(enabled),
                1004 => self.modes.focus_reporting = enabled,
                1000 | 1002 | 1003 | 1006 => self.modes.mouse_reporting = enabled,
                2004 => self.modes.bracketed_paste = enabled,
                _ => self.unsupported(),
            }
        }
    }

    fn discarded(&mut self) {
        self.diagnostics.discarded_sequences =
            self.diagnostics.discarded_sequences.saturating_add(1);
    }

    fn unsupported(&mut self) {
        self.diagnostics.unsupported_sequences =
            self.diagnostics.unsupported_sequences.saturating_add(1);
    }

    fn apply_hyperlink(&mut self, params: &[&[u8]]) {
        let Some(uri) = params.get(2) else {
            self.discarded();
            return;
        };
        if uri.is_empty() {
            self.pen.hyperlink = None;
            return;
        }
        let Ok(uri) = std::str::from_utf8(uri) else {
            self.discarded();
            return;
        };
        if self.hyperlink_targets.len() >= 4096 {
            self.discarded();
            return;
        }
        let id = HyperlinkId(self.next_hyperlink_id);
        self.next_hyperlink_id = self.next_hyperlink_id.saturating_add(1);
        self.hyperlink_targets.insert(id, uri.to_owned());
        self.pen.hyperlink = Some(id);
    }
}

fn flatten_params(params: &Params) -> Vec<u16> {
    params
        .iter()
        .flat_map(|subparams| subparams.iter().copied())
        .collect()
}

fn extended_color(params: &[u16]) -> Option<(Color, usize)> {
    match params {
        [5, index, ..] => Some((Color::Indexed(u8::try_from(*index).ok()?), 2)),
        [2, red, green, blue, ..] => Some((
            Color::Rgb(
                u8::try_from(*red).ok()?,
                u8::try_from(*green).ok()?,
                u8::try_from(*blue).ok()?,
            ),
            4,
        )),
        _ => None,
    }
}

fn param(params: &[u16], index: usize, default: u16) -> u16 {
    params
        .get(index)
        .copied()
        .filter(|value| *value != 0)
        .unwrap_or(default)
}
