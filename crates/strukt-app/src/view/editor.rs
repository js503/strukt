use iced::widget::{Space, column, row, text};
use iced::{Alignment, Element, Fill, Length};
use strukt_shell::CanvasLayout;
use strukt_ui::UiTheme;

use crate::app::{EDITOR_SUPPORTING_SURFACE_ID, Message, StruktApp};

pub(super) fn is_primary_canvas(app: &StruktApp) -> bool {
    app.shell.canvas.primary().as_str() == EDITOR_SUPPORTING_SURFACE_ID
}

pub(super) fn is_secondary_canvas(app: &StruktApp) -> bool {
    matches!(
        &app.shell.canvas,
        CanvasLayout::Split { secondary, .. }
            if secondary.as_str() == EDITOR_SUPPORTING_SURFACE_ID
    )
}

pub(super) fn is_drawer(app: &StruktApp) -> bool {
    app.shell
        .drawer
        .surface
        .as_ref()
        .is_some_and(|surface| surface.as_str() == EDITOR_SUPPORTING_SURFACE_ID)
}

pub(super) fn canvas<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if !is_primary_canvas(app) && !is_secondary_canvas(app) {
        return super::editor_canvas(app, theme);
    }
    let controls = row![
        text("Editor").size(12),
        Space::new().width(Fill),
        strukt_ui::quiet_button(
            "Return to drawer",
            Some(Message::DemoteSupportingEditorToDrawer),
            theme,
        )
        .width(Length::Shrink),
        strukt_ui::quiet_button("×", Some(Message::CloseSupportingEditorPlacement), theme,)
            .width(Length::Shrink),
    ]
    .height(super::layout::TERMINAL_DRAWER_HEADER_HEIGHT)
    .align_y(Alignment::Center)
    .spacing(theme.metrics.space_2)
    .padding([0.0, theme.metrics.space_3]);

    strukt_ui::chrome(
        column![controls, super::editor_canvas(app, theme)].spacing(0),
        theme,
        strukt_ui::ChromeRole::Panel,
    )
    .height(Fill)
    .width(Fill)
    .into()
}

pub(super) fn drawer<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    strukt_ui::chrome(
        super::editor_surface(app, theme, true),
        theme,
        strukt_ui::ChromeRole::Panel,
    )
    .height(super::layout::EDITOR_SUPPORTING_DRAWER_HEIGHT)
    .width(Fill)
    .into()
}
