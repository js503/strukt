use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Background, Border, Color, Element, Fill, Length};
use strukt_core::CapabilityId;
use strukt_shell::Activity;
use strukt_theme::{Rgb, ThemeTokens};

use crate::app::{Message, StruktApp};

fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

fn panel_style(tokens: ThemeTokens, background: Rgb) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color(background))),
        text_color: Some(color(tokens.text_primary)),
        border: Border {
            color: color(tokens.border),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn activity_button(label: &'static str, activity: Activity) -> Element<'static, Message> {
    button(text(label))
        .width(Fill)
        .on_press(Message::SelectActivity(activity))
        .into()
}

pub fn view(app: &StruktApp) -> Element<'_, Message> {
    let tokens = ThemeTokens::builtin(app.shell.theme_mode);
    let body = row![
        activity_rail(tokens),
        explorer(app, tokens),
        primary_canvas(tokens),
        context_panel(app, tokens),
    ]
    .height(Fill);

    container(column![header(tokens), body, drawer(app, tokens)].height(Fill))
        .width(Fill)
        .height(Fill)
        .style(panel_style(tokens, tokens.canvas))
        .into()
}

fn header(tokens: ThemeTokens) -> Element<'static, Message> {
    container(
        row![
            text("strukt").size(16),
            text("  /  local workspace").size(13),
            Space::new().width(Fill),
            button("Toggle theme").on_press(Message::ToggleTheme),
            button("Context").on_press(Message::ToggleContext),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Fill)
    .style(panel_style(tokens, tokens.panel))
    .into()
}

fn activity_rail(tokens: ThemeTokens) -> Element<'static, Message> {
    container(
        column![
            activity_button("Files", Activity::Files),
            activity_button("Search", Activity::Search),
            activity_button("Git", Activity::SourceControl),
            activity_button("Sessions", Activity::Sessions),
            activity_button("Tasks", Activity::Tasks),
            activity_button("Connect", Activity::Connections),
            activity_button("Extend", Activity::Extensions),
            Space::new().height(Fill),
            activity_button("Settings", Activity::Settings),
        ]
        .spacing(6),
    )
    .padding(6)
    .width(Length::Fixed(92.0))
    .style(panel_style(tokens, tokens.panel))
    .into()
}

fn explorer(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    if !app.shell.explorer_visible {
        return container(Space::new()).width(Length::Shrink).into();
    }

    container(
        column![
            row![
                text("EXPLORER"),
                Space::new().width(Fill),
                button("×").on_press(Message::ToggleExplorer),
            ],
            text("STRUKT"),
            scrollable(
                column![
                    text("▾ crates"),
                    text("  ▸ strukt-app"),
                    text("  ▸ strukt-core"),
                    text("  ▸ strukt-shell"),
                    text("  ▸ strukt-theme"),
                    text("▸ docs"),
                    text("  README.md"),
                    text("  Cargo.toml"),
                ]
                .spacing(6)
            ),
        ]
        .spacing(10),
    )
    .padding(10)
    .width(Length::Fixed(235.0))
    .style(panel_style(tokens, tokens.panel))
    .into()
}

fn primary_canvas(tokens: ThemeTokens) -> Element<'static, Message> {
    container(
        column![
            text("Workspace shell").size(22),
            text("The primary canvas adapts to files, terminals, logs, and tools."),
            Space::new().height(Fill),
            text("Open a file or choose an activity to begin."),
        ]
        .spacing(10),
    )
    .padding(20)
    .width(Fill)
    .height(Fill)
    .style(panel_style(tokens, tokens.canvas))
    .into()
}

fn context_panel(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    if !app.shell.context_visible {
        return container(Space::new()).width(Length::Shrink).into();
    }

    let ai_status = if app.capabilities.is_enabled(CapabilityId::AI) {
        "AI · WORKSPACE CONTEXT"
    } else {
        "WORKSPACE CONTEXT"
    };

    container(
        column![
            text(ai_status),
            text("Current workspace"),
            text("4 capabilities enabled"),
            Space::new().height(Fill),
            button("Hide context").on_press(Message::ToggleContext),
        ]
        .spacing(10),
    )
    .padding(10)
    .width(Length::Fixed(250.0))
    .style(panel_style(tokens, tokens.panel))
    .into()
}

fn drawer(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    if !app.shell.drawer_visible {
        return button("Open terminal drawer")
            .on_press(Message::ToggleDrawer)
            .width(Fill)
            .into();
    }

    container(
        row![
            text("TERMINAL  ·  local shell foundation"),
            Space::new().width(Fill),
            button("Close").on_press(Message::ToggleDrawer),
        ]
        .spacing(8),
    )
    .padding(10)
    .height(Length::Fixed(130.0))
    .style(panel_style(tokens, tokens.terminal_background))
    .into()
}
