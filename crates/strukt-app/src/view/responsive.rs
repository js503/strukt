#[expect(
    clippy::struct_excessive_bools,
    reason = "the composition contract intentionally describes four independently visible regions"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResponsiveComposition {
    pub activity_rail: bool,
    pub sidebar: bool,
    pub canvas: bool,
    pub context: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResponsivePolicy {
    sidebar_min_width: u32,
    context_min_width: u32,
}

impl Default for ResponsivePolicy {
    fn default() -> Self {
        Self {
            sidebar_min_width: 1_120,
            context_min_width: 1_560,
        }
    }
}

impl ResponsivePolicy {
    pub const fn compose(
        self,
        width: u32,
        sidebar_requested: bool,
        context_requested: bool,
    ) -> ResponsiveComposition {
        ResponsiveComposition {
            activity_rail: true,
            sidebar: sidebar_requested && width >= self.sidebar_min_width,
            canvas: true,
            context: context_requested && width >= self.context_min_width,
        }
    }

    pub const fn reduced_motion() -> bool {
        true
    }

    pub const fn transition_duration_ms() -> u16 {
        0
    }
}
