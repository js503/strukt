use iced::widget::{Space, column, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length};
use strukt_ui::{ChromeRole, StateKind, UiTheme, chrome, quiet_button, state_panel};

use crate::app::{Message, StruktApp};
use crate::language::{DiagnosticSeverity, ProblemFilter};

pub(super) fn drawer(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    let counts = app.language.problem_counts();
    let mut problems = column![].spacing(theme.metrics.space_1);
    for problem in app.language.visible_problems() {
        let severity = match problem.severity() {
            DiagnosticSeverity::Error => "ERROR",
            DiagnosticSeverity::Warning => "WARNING",
            DiagnosticSeverity::Information => "INFO",
            DiagnosticSeverity::Hint => "HINT",
        };
        let source = problem
            .source()
            .map_or_else(String::new, |source| format!(" · {source}"));
        let label = format!(
            "{severity}  {}:{}:{}  {}{source}",
            problem.path().display(),
            problem.line() + 1,
            problem.character() + 1,
            problem.message(),
        );
        problems = problems.push(strukt_ui::list_row_owned(
            label,
            false,
            Some(Message::OpenProblem {
                id: problem.document_id(),
                line: problem.line(),
                character: problem.character(),
            }),
            theme,
        ));
    }
    if app.language.visible_problems().is_empty() {
        problems = problems.push(state_panel(
            "No problems",
            "No diagnostics in synchronized files.",
            StateKind::Success,
            theme,
        ));
    }
    let header = row![
        text("PROBLEMS").size(11),
        text(format!(
            "{} errors · {} warnings · {} info · {} hints",
            counts.errors, counts.warnings, counts.information, counts.hints
        ))
        .size(11),
        Space::new().width(Fill),
        quiet_button(
            "All",
            Some(Message::SetProblemFilter(ProblemFilter::All)),
            theme
        ),
        quiet_button(
            "Errors",
            Some(Message::SetProblemFilter(ProblemFilter::Errors)),
            theme,
        ),
        quiet_button(
            "Warnings",
            Some(Message::SetProblemFilter(ProblemFilter::Warnings)),
            theme,
        ),
        quiet_button("Hide", Some(Message::ToggleProblems), theme),
    ]
    .spacing(theme.metrics.space_2)
    .align_y(Alignment::Center);
    chrome(
        column![header, scrollable(problems).height(Fill)].spacing(theme.metrics.space_2),
        theme,
        ChromeRole::ActivePanel,
    )
    .padding(theme.metrics.space_2)
    .height(Length::Fixed(f32::from(app.shell.drawer.height)))
    .into()
}
