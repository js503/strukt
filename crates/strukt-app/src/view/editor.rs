use iced::Element;
use strukt_ui::UiTheme;

use crate::app::{Message, StruktApp};

pub(super) fn canvas<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    super::editor_canvas(app, theme)
}
