use std::collections::BTreeSet;

use strukt_theme::{ThemeId, ThemeMode};

use crate::{
    Activity, ActivityContribution, CanvasLayout, DrawerState, FocusRegion, PanelState,
    PromotionState, SurfaceId, clamp_split_ratio,
};

#[derive(Clone, Debug, PartialEq)]
pub enum ShellAction {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
    SetThemeMode(ThemeMode),
    OpenDrawer(SurfaceId),
    PromoteDrawerToSplit { ratio: f32 },
    PromoteDrawerToFull,
    DemotePromotedSurface,
    Focus(FocusRegion),
}

#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "legacy visibility fields remain public until Slice 2 migrates all app callers"
)]
pub struct ShellState {
    pub active_activity: Activity,
    pub explorer_visible: bool,
    pub context_visible: bool,
    pub drawer_visible: bool,
    pub theme_mode: ThemeMode,
    pub theme_id: ThemeId,
    pub sidebar: PanelState,
    pub context: PanelState,
    pub canvas: CanvasLayout,
    pub drawer: DrawerState,
    pub focus_region: FocusRegion,
    pub reduced_motion: bool,
    promotion: Option<PromotionState>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            active_activity: Activity::Files,
            explorer_visible: true,
            context_visible: false,
            drawer_visible: false,
            theme_mode: ThemeMode::Dark,
            theme_id: ThemeId::quiet_precision(),
            sidebar: PanelState {
                visible: true,
                surface: Activity::Files.sidebar_surface(),
                width: 256,
            },
            context: PanelState {
                visible: false,
                surface: None,
                width: 320,
            },
            canvas: CanvasLayout::Single {
                primary: Activity::Files.canvas_surface(),
            },
            drawer: DrawerState {
                surface: Some(SurfaceId::trusted("terminal.local.primary")),
                visible: false,
                height: 280,
            },
            focus_region: FocusRegion::Canvas,
            reduced_motion: true,
            promotion: None,
        }
    }
}

impl ShellState {
    pub fn apply(&mut self, action: ShellAction) {
        match action {
            ShellAction::SelectActivity(activity) => self.select_activity(activity),
            ShellAction::ToggleContext => {
                self.context.visible = !self.context.visible;
                self.context_visible = self.context.visible;
                if !self.context.visible && self.focus_region == FocusRegion::ContextPanel {
                    self.focus_region = FocusRegion::Canvas;
                }
            }
            ShellAction::ToggleDrawer => {
                self.drawer.visible = !self.drawer.visible;
                self.drawer_visible = self.drawer.visible;
                if !self.drawer.visible && self.focus_region == FocusRegion::Drawer {
                    self.focus_region = FocusRegion::Canvas;
                }
            }
            ShellAction::ToggleExplorer => {
                self.explorer_visible = !self.explorer_visible;
                self.sidebar.visible = self.explorer_visible;
                if !self.sidebar.visible && self.focus_region == FocusRegion::Sidebar {
                    self.focus_region = FocusRegion::Canvas;
                }
            }
            ShellAction::ToggleTheme => {
                self.theme_mode = match self.theme_mode {
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::Dark => ThemeMode::Light,
                };
            }
            ShellAction::SetThemeMode(mode) => self.theme_mode = mode,
            ShellAction::OpenDrawer(surface) => {
                self.drawer.surface = Some(surface);
                self.drawer.visible = true;
                self.drawer_visible = true;
            }
            ShellAction::PromoteDrawerToSplit { ratio } => self.promote_drawer(Some(ratio)),
            ShellAction::PromoteDrawerToFull => self.promote_drawer(None),
            ShellAction::DemotePromotedSurface => self.demote_promoted_surface(),
            ShellAction::Focus(region) => self.focus_region = region,
        }
    }

    pub(crate) fn select_contribution(&mut self, contribution: &ActivityContribution) {
        self.active_activity = contribution.activity;
        self.sidebar.surface.clone_from(&contribution.sidebar);
        self.sidebar.visible = contribution.sidebar.is_some();
        self.explorer_visible = self.sidebar.visible;
        self.canvas = CanvasLayout::Single {
            primary: contribution.canvas.clone(),
        };
        self.promotion = None;
        self.focus_region = FocusRegion::Canvas;
    }

    pub(crate) fn remove_surfaces(&mut self, removed: &BTreeSet<SurfaceId>) {
        if self
            .sidebar
            .surface
            .as_ref()
            .is_some_and(|surface| removed.contains(surface))
        {
            self.sidebar.surface = None;
            self.sidebar.visible = false;
            self.explorer_visible = false;
        }
        if self
            .context
            .surface
            .as_ref()
            .is_some_and(|surface| removed.contains(surface))
        {
            self.context.surface = None;
            self.context.visible = false;
            self.context_visible = false;
        }
        if self
            .drawer
            .surface
            .as_ref()
            .is_some_and(|surface| removed.contains(surface))
        {
            self.drawer.surface = None;
            self.drawer.visible = false;
            self.drawer_visible = false;
        }
        if self.canvas_contains_any(removed) {
            self.canvas = CanvasLayout::Single {
                primary: self.active_activity.canvas_surface(),
            };
        }
        if self
            .promotion
            .as_ref()
            .is_some_and(|promotion| removed.contains(&promotion.surface))
        {
            self.promotion = None;
        }
        if matches!(
            self.focus_region,
            FocusRegion::Sidebar | FocusRegion::ContextPanel | FocusRegion::Drawer
        ) {
            self.focus_region = FocusRegion::Canvas;
        }
    }

    fn select_activity(&mut self, activity: Activity) {
        self.active_activity = activity;
        self.sidebar.surface = activity.sidebar_surface();
        self.sidebar.visible = true;
        self.explorer_visible = true;
        self.canvas = CanvasLayout::Single {
            primary: activity.canvas_surface(),
        };
        self.promotion = None;
        self.focus_region = FocusRegion::Canvas;
    }

    fn promote_drawer(&mut self, split_ratio: Option<f32>) {
        let Some(surface) = self.drawer.surface.clone() else {
            return;
        };
        if self.promotion.is_some() {
            return;
        }
        let prior_canvas = self.canvas.clone();
        self.canvas = split_ratio.map_or_else(
            || CanvasLayout::Single {
                primary: surface.clone(),
            },
            |ratio| CanvasLayout::Split {
                primary: prior_canvas.primary().clone(),
                secondary: surface.clone(),
                ratio: clamp_split_ratio(ratio),
            },
        );
        self.promotion = Some(PromotionState {
            prior_canvas,
            surface,
        });
        self.drawer.visible = false;
        self.drawer_visible = false;
        self.focus_region = FocusRegion::Canvas;
    }

    fn demote_promoted_surface(&mut self) {
        let Some(promotion) = self.promotion.take() else {
            return;
        };
        self.canvas = promotion.prior_canvas;
        self.drawer.surface = Some(promotion.surface);
        self.drawer.visible = true;
        self.drawer_visible = true;
        self.focus_region = FocusRegion::Drawer;
    }

    fn canvas_contains_any(&self, surfaces: &BTreeSet<SurfaceId>) -> bool {
        surfaces.iter().any(|surface| self.canvas.contains(surface))
    }
}
