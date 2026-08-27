use iced::Element;
use strukt_ui::UiTheme;

use crate::app::{Message, StruktApp};

pub(super) fn sidebar<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    super::explorer(app, theme)
}
