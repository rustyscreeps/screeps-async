//! Errors emitted by screeps-async
use std::fmt::{Debug, Display, Formatter};

/// An Error returned by the [crate::runtime::ScreepsRuntime]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The allocated time this tick has already been consumed
    OutOfTime,
    /// The runtime has detected that deadlock has occurred.
    /// This usually means you tried to [block_on](crate::block_on) a future that [delay](crate::time::delay_ticks)s
    /// across ticks
    DeadlockDetected,
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::OutOfTime => {
                write!(f, "Ran out of allocated time this tick")
            }
            RuntimeError::DeadlockDetected => {
                write!(f, "Async runtime has been deadlocked")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}
