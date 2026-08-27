use strukt_shell::{
    Activity, ActivityContribution, CanvasLayout, CommandContribution, CommandId,
    ContributionError, ContributionRegistry, ExecutionBoundary, FocusRegion, ShellAction,
    ShellContribution, ShellState, SurfaceContribution, SurfaceId,
};

fn surface(id: &str) -> SurfaceId {
    SurfaceId::new(id).expect("valid surface id")
}

#[test]
fn activity_selection_updates_contextual_sidebar_and_canvas_owner() {
    let mut state = ShellState::default();
    state.apply(ShellAction::SelectActivity(Activity::Search));

    assert_eq!(state.active_activity, Activity::Search);
    assert_eq!(state.sidebar.surface, Some(surface("search.sidebar")));
    assert_eq!(
        state.canvas,
        CanvasLayout::Single {
            primary: surface("search"),
        }
    );
}

#[test]
fn drawer_promotion_preserves_surface_identity_and_demotes_to_prior_canvas() {
    let mut state = ShellState::default();
    let terminal = surface("terminal.local.primary");
    let original = state.canvas.clone();

    state.apply(ShellAction::OpenDrawer(terminal.clone()));
    assert_eq!(state.canvas, original);
    assert_eq!(state.drawer.surface, Some(terminal.clone()));

    state.apply(ShellAction::PromoteDrawerToSplit { ratio: 0.95 });
    assert_eq!(
        state.canvas,
        CanvasLayout::Split {
            primary: surface("files"),
            secondary: terminal.clone(),
            ratio: 0.8,
        }
    );
    assert!(!state.drawer.visible);

    state.apply(ShellAction::DemotePromotedSurface);
    assert_eq!(state.canvas, original);
    assert_eq!(state.drawer.surface, Some(terminal));
    assert!(state.drawer.visible);
}

#[test]
fn full_promotion_and_panel_closure_return_focus_without_runtime_replacement() {
    let mut state = ShellState::default();
    let problems = surface("problems");
    state.apply(ShellAction::OpenDrawer(problems.clone()));
    state.apply(ShellAction::Focus(FocusRegion::Drawer));
    state.apply(ShellAction::PromoteDrawerToFull);

    assert_eq!(state.canvas, CanvasLayout::Single { primary: problems });
    assert_eq!(state.focus_region, FocusRegion::Canvas);

    state.apply(ShellAction::Focus(FocusRegion::ContextPanel));
    state.apply(ShellAction::ToggleContext);
    assert_eq!(state.focus_region, FocusRegion::Canvas);
}

#[test]
fn duplicate_contributions_are_rejected_and_removal_sanitizes_shell_state() {
    let files = contribution("files", Activity::Files, "files");
    let sessions = contribution("sessions", Activity::Sessions, "sessions");
    let mut registry = ContributionRegistry::default();
    registry.register(files).expect("register files");
    registry
        .register(sessions.clone())
        .expect("register sessions");
    assert!(matches!(
        registry.register(sessions),
        Err(ContributionError::DuplicateContribution(_))
    ));

    let mut state = ShellState::default();
    state.apply(ShellAction::SelectActivity(Activity::Sessions));
    registry
        .unregister("sessions", &mut state)
        .expect("unregister sessions");

    assert_eq!(state.active_activity, Activity::Files);
    assert_eq!(
        state.canvas,
        CanvasLayout::Single {
            primary: surface("files"),
        }
    );
}

fn contribution(id: &str, activity: Activity, surface_id: &str) -> ShellContribution {
    ShellContribution {
        id: id.to_owned(),
        activity: Some(ActivityContribution {
            activity,
            label: id.to_owned(),
            sidebar: Some(surface(&format!("{surface_id}.sidebar"))),
            canvas: surface(surface_id),
            order: if activity == Activity::Files { 0 } else { 10 },
        }),
        surfaces: vec![SurfaceContribution {
            id: surface(surface_id),
        }],
        commands: vec![CommandContribution {
            id: CommandId(format!("{id}.open")),
            title: format!("Open {id}"),
            category: "Navigation".to_owned(),
            keywords: Vec::new(),
            shortcut: None,
            boundary: ExecutionBoundary::Interface,
            enabled: true,
        }],
    }
}
