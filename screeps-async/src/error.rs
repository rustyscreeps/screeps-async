//! Errors emitted by screeps-async
use std::fmt::{Debug, Display, Formatter};

/// An Error returned by the [crate::runtime::ScreepsRuntime]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum RuntimeError {
    /// The allocated time this tick has already been consumed
    OutOfTime,
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::OutOfTime => {
                write!(f, "Ran out of allocated time this tick")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}
