#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeTokens {
    pub canvas: Rgb,
    pub panel: Rgb,
    pub panel_active: Rgb,
    pub border: Rgb,
    pub text_primary: Rgb,
    pub text_muted: Rgb,
    pub accent: Rgb,
    pub focus: Rgb,
    pub terminal_background: Rgb,
    pub connection_remote: Rgb,
    pub status_success: Rgb,
    pub status_warning: Rgb,
}

impl ThemeTokens {
    #[must_use]
    pub const fn builtin(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self {
                canvas: Rgb::new(246, 247, 249),
                panel: Rgb::new(255, 255, 255),
                panel_active: Rgb::new(235, 240, 247),
                border: Rgb::new(207, 213, 223),
                text_primary: Rgb::new(27, 35, 48),
                text_muted: Rgb::new(95, 104, 119),
                accent: Rgb::new(31, 111, 235),
                focus: Rgb::new(31, 111, 235),
                terminal_background: Rgb::new(13, 17, 23),
                connection_remote: Rgb::new(35, 134, 54),
                status_success: Rgb::new(35, 134, 54),
                status_warning: Rgb::new(154, 103, 0),
            },
            ThemeMode::Dark => Self {
                canvas: Rgb::new(13, 17, 23),
                panel: Rgb::new(22, 27, 34),
                panel_active: Rgb::new(33, 38, 45),
                border: Rgb::new(48, 54, 61),
                text_primary: Rgb::new(240, 246, 252),
                text_muted: Rgb::new(139, 148, 158),
                accent: Rgb::new(88, 166, 255),
                focus: Rgb::new(88, 166, 255),
                terminal_background: Rgb::new(9, 12, 16),
                connection_remote: Rgb::new(126, 231, 135),
                status_success: Rgb::new(126, 231, 135),
                status_warning: Rgb::new(227, 179, 65),
            },
        }
    }
}
