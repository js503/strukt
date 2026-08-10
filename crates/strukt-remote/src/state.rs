use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_DETAIL_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independently negotiated remote capabilities remain explicit"
)]
pub struct ConnectionCapabilities {
    pub files: bool,
    pub search: bool,
    pub git: bool,
    pub processes: bool,
    pub language: bool,
    pub watches: bool,
}

impl ConnectionCapabilities {
    #[must_use]
    pub const fn all() -> Self {
        Self {
            files: true,
            search: true,
            git: true,
            processes: true,
            language: true,
            watches: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ConnectionPhase {
    Disconnected,
    Connecting,
    TerminalOnly,
    NegotiatingHelper,
    Ready,
    Stale,
    Reconnecting,
    Failed,
    Disconnecting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RecoveryAction {
    InstallHelper,
    RetryNow,
    Disconnect,
    OpenTerminal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectionProjection {
    pub phase: ConnectionPhase,
    pub generation: u64,
    pub capabilities: ConnectionCapabilities,
    pub detail: Option<String>,
    pub recovery: Vec<RecoveryAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    base: Duration,
    maximum: Duration,
    max_attempts: u32,
}

impl RetryPolicy {
    /// Creates a capped exponential retry policy.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidRetryPolicy`] when durations are zero, the
    /// maximum is below the base, or no attempt is allowed.
    pub fn new(base: Duration, maximum: Duration, max_attempts: u32) -> Result<Self, StateError> {
        if base.is_zero() || maximum.is_zero() || maximum < base || max_attempts == 0 {
            return Err(StateError::InvalidRetryPolicy);
        }
        Ok(Self {
            base,
            maximum,
            max_attempts,
        })
    }

    fn delay(self, attempt: u32) -> Duration {
        let multiplier = 1_u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
        self.base.saturating_mul(multiplier).min(self.maximum)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(250),
            maximum: Duration::from_secs(8),
            max_attempts: 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum StateError {
    #[error("the connection transition is invalid for the current state")]
    InvalidTransition,
    #[error("the retry policy is invalid")]
    InvalidRetryPolicy,
    #[error("the reconnect retry limit was reached")]
    RetryLimitReached,
}

#[derive(Clone, Debug)]
pub struct ConnectionMachine {
    phase: ConnectionPhase,
    generation: u64,
    capabilities: ConnectionCapabilities,
    detail: Option<String>,
    retry_policy: RetryPolicy,
    retry_attempts: u32,
}

impl ConnectionMachine {
    #[must_use]
    pub const fn new(retry_policy: RetryPolicy) -> Self {
        Self {
            phase: ConnectionPhase::Disconnected,
            generation: 0,
            capabilities: ConnectionCapabilities {
                files: false,
                search: false,
                git: false,
                processes: false,
                language: false,
                watches: false,
            },
            detail: None,
            retry_policy,
            retry_attempts: 0,
        }
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn accepts_generation(&self, generation: u64) -> bool {
        self.generation == generation
    }

    #[must_use]
    pub fn projection(&self) -> ConnectionProjection {
        ConnectionProjection {
            phase: self.phase,
            generation: self.generation,
            capabilities: self.capabilities,
            detail: self.detail.clone(),
            recovery: recovery_actions(self.phase),
        }
    }

    /// Starts an explicit connection attempt.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] unless disconnected or failed.
    pub fn connect(&mut self) -> Result<(), StateError> {
        self.require(&[ConnectionPhase::Disconnected, ConnectionPhase::Failed])?;
        self.generation = self.generation.saturating_add(1);
        self.retry_attempts = 0;
        self.capabilities = ConnectionCapabilities::default();
        self.set(ConnectionPhase::Connecting, None);
        Ok(())
    }

    /// Records that direct interactive SSH is usable.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] when no connection is active.
    pub fn terminal_available(&mut self) -> Result<(), StateError> {
        self.require(&[ConnectionPhase::Connecting, ConnectionPhase::Reconnecting])?;
        self.set(ConnectionPhase::TerminalOnly, None);
        Ok(())
    }

    /// Starts helper negotiation from terminal-only mode.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] unless terminal fallback exists.
    pub fn negotiate_helper(&mut self) -> Result<(), StateError> {
        self.require(&[ConnectionPhase::TerminalOnly])?;
        self.set(ConnectionPhase::NegotiatingHelper, None);
        Ok(())
    }

    /// Publishes negotiated helper capabilities.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] outside terminal-only or helper
    /// negotiation state.
    pub fn helper_ready(&mut self, capabilities: ConnectionCapabilities) -> Result<(), StateError> {
        self.require(&[
            ConnectionPhase::TerminalOnly,
            ConnectionPhase::NegotiatingHelper,
        ])?;
        self.capabilities = capabilities;
        self.retry_attempts = 0;
        self.set(ConnectionPhase::Ready, None);
        Ok(())
    }

    /// Marks the last immutable snapshot stale after transport loss.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] when no transport is active.
    pub fn transport_lost(&mut self, detail: impl AsRef<str>) -> Result<(), StateError> {
        self.require(&[
            ConnectionPhase::Connecting,
            ConnectionPhase::TerminalOnly,
            ConnectionPhase::NegotiatingHelper,
            ConnectionPhase::Ready,
            ConnectionPhase::Reconnecting,
        ])?;
        self.set(ConnectionPhase::Stale, Some(detail.as_ref()));
        Ok(())
    }

    /// Begins the next bounded reconnect attempt and returns its delay.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] unless stale, or
    /// [`StateError::RetryLimitReached`] after the configured limit.
    pub fn begin_retry(&mut self) -> Result<Duration, StateError> {
        self.require(&[ConnectionPhase::Stale])?;
        if self.retry_attempts >= self.retry_policy.max_attempts {
            self.set(ConnectionPhase::Failed, self.detail.clone().as_deref());
            return Err(StateError::RetryLimitReached);
        }
        let delay = self.retry_policy.delay(self.retry_attempts);
        self.retry_attempts += 1;
        self.set(
            ConnectionPhase::Reconnecting,
            self.detail.clone().as_deref(),
        );
        Ok(delay)
    }

    /// Records a failed reconnect while retaining stale capabilities.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] unless reconnecting.
    pub fn retry_failed(&mut self, detail: impl AsRef<str>) -> Result<(), StateError> {
        self.require(&[ConnectionPhase::Reconnecting])?;
        self.set(ConnectionPhase::Stale, Some(detail.as_ref()));
        Ok(())
    }

    /// Accepts a new transport generation after reconnect.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] unless reconnecting.
    pub fn retry_connected(&mut self) -> Result<(), StateError> {
        self.require(&[ConnectionPhase::Reconnecting])?;
        self.generation = self.generation.saturating_add(1);
        self.retry_attempts = 0;
        self.capabilities = ConnectionCapabilities::default();
        self.set(ConnectionPhase::TerminalOnly, None);
        Ok(())
    }

    /// Records an actionable connection failure.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] when already disconnected or
    /// disconnecting.
    pub fn fail(&mut self, detail: impl AsRef<str>) -> Result<(), StateError> {
        if matches!(
            self.phase,
            ConnectionPhase::Disconnected | ConnectionPhase::Disconnecting
        ) {
            return Err(StateError::InvalidTransition);
        }
        self.set(ConnectionPhase::Failed, Some(detail.as_ref()));
        Ok(())
    }

    /// Starts explicit disconnect and cancels future retry state.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] when already disconnected or
    /// disconnecting.
    pub fn disconnect(&mut self) -> Result<(), StateError> {
        if matches!(
            self.phase,
            ConnectionPhase::Disconnected | ConnectionPhase::Disconnecting
        ) {
            return Err(StateError::InvalidTransition);
        }
        self.retry_attempts = 0;
        self.set(ConnectionPhase::Disconnecting, None);
        Ok(())
    }

    /// Completes explicit disconnect and clears remote capabilities.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidTransition`] unless disconnecting.
    pub fn disconnected(&mut self) -> Result<(), StateError> {
        self.require(&[ConnectionPhase::Disconnecting])?;
        self.capabilities = ConnectionCapabilities::default();
        self.set(ConnectionPhase::Disconnected, None);
        Ok(())
    }

    fn require(&self, expected: &[ConnectionPhase]) -> Result<(), StateError> {
        if expected.contains(&self.phase) {
            Ok(())
        } else {
            Err(StateError::InvalidTransition)
        }
    }

    fn set(&mut self, phase: ConnectionPhase, detail: Option<&str>) {
        self.phase = phase;
        self.detail = detail.map(bounded_detail);
    }
}

fn recovery_actions(phase: ConnectionPhase) -> Vec<RecoveryAction> {
    match phase {
        ConnectionPhase::TerminalOnly => vec![RecoveryAction::InstallHelper],
        ConnectionPhase::Stale => vec![RecoveryAction::RetryNow, RecoveryAction::Disconnect],
        ConnectionPhase::Failed => vec![
            RecoveryAction::RetryNow,
            RecoveryAction::OpenTerminal,
            RecoveryAction::Disconnect,
        ],
        _ => Vec::new(),
    }
}

fn bounded_detail(detail: &str) -> String {
    let sanitized = detail.replace('\0', "�");
    if sanitized.len() <= MAX_DETAIL_BYTES {
        return sanitized;
    }
    let mut end = MAX_DETAIL_BYTES;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_owned()
}
