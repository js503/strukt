use strukt_theme::ThemeMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activity {
    Files,
    Search,
    SourceControl,
    Sessions,
    Tasks,
    Connections,
    Extensions,
    Settings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellAction {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellState {
    pub active_activity: Activity,
    pub explorer_visible: bool,
    pub context_visible: bool,
    pub drawer_visible: bool,
    pub theme_mode: ThemeMode,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            active_activity: Activity::Files,
            explorer_visible: true,
            context_visible: true,
            drawer_visible: false,
            theme_mode: ThemeMode::Dark,
        }
    }
}

impl ShellState {
    pub fn apply(&mut self, action: ShellAction) {
        match action {
            ShellAction::SelectActivity(activity) => {
                self.active_activity = activity;
                if activity == Activity::Files {
                    self.explorer_visible = true;
                }
            }
            ShellAction::ToggleContext => self.context_visible = !self.context_visible,
            ShellAction::ToggleDrawer => self.drawer_visible = !self.drawer_visible,
            ShellAction::ToggleExplorer => self.explorer_visible = !self.explorer_visible,
            ShellAction::ToggleTheme => {
                self.theme_mode = match self.theme_mode {
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::Dark => ThemeMode::Light,
                };
            }
        }
    }
}
