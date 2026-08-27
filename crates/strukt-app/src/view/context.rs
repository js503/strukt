use iced::Element;
use strukt_ui::UiTheme;

use crate::app::{Message, StruktApp};

pub(super) fn panel(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    super::context_panel(app, theme)
}
