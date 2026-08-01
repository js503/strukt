use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

static NEXT_PANE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

macro_rules! terminal_id {
    ($name:ident, $counter:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        pub struct $name(u64);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self($counter.fetch_add(1, Ordering::Relaxed))
            }

            #[must_use]
            pub const fn value(self) -> u64 {
                self.0
            }

            pub(crate) fn reserve_after(self) {
                let next = self.0.saturating_add(1);
                let mut current = $counter.load(Ordering::Relaxed);
                while current < next {
                    match $counter.compare_exchange_weak(
                        current,
                        next,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(observed) => current = observed,
                    }
                }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

terminal_id!(TerminalPaneId, NEXT_PANE_ID);
terminal_id!(TerminalTabId, NEXT_TAB_ID);
