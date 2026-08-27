use iced::Element;
use strukt_ui::{StateKind, UiTheme, state_panel};

use crate::app::Message;

pub(super) fn empty<'a>(title: &'a str, detail: &'a str, theme: &UiTheme) -> Element<'a, Message> {
    state_panel(title, detail, StateKind::Empty, theme)
}
