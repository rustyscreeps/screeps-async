#![warn(missing_docs)]
//! A tick-aware async runtime for Screeps
//!
//! # Getting Started
//!
//! Add `screeps-async` to your `Cargo.toml`
//! ```toml
//! [dependencies]
//! screeps-async = "0.1.0"
//! ```
//!
//! # The [#[screeps_async::main]][screeps_async::main] macro
//! ```
//! #[screeps_async::main]
//! pub fn game_loop() {
//!     // Tick logic that spawns some tasks
//!     screeps_async::spawn(async {
//!         println!("Hello!");
//!     });
//! }
//! ```
//!
//! This expands to roughly the following:
//!
//! ```
//! pub fn game_loop() {
//!     // Tick logic that spawns some tasks
//!     screeps_async::spawn(async {
//!         println!("Hello!");
//!     });
//!
//!     screeps_async::run().unwrap();
//! }
//! ```

pub mod macros;

pub use macros::*;
use std::cell::RefCell;
pub mod error;
pub mod job;
pub mod runtime;
pub mod time;

use crate::error::RuntimeError;
use crate::job::JobHandle;
use crate::runtime::{Builder, ScreepsRuntime};
use std::future::Future;

thread_local! {
    /// The current runtime
    pub static CURRENT: RefCell<Option<ScreepsRuntime>> =
        const { RefCell::new(None) };
}

/// Configures the runtime with default settings. Must be called only once
///
/// To use custom settings, create a [Builder] with [Builder::new], customize as needed,
/// then call [Builder::apply]
///
/// # Panics
///
/// This function panics if there is already a runtime initialized
pub fn initialize() {
    Builder::new().apply()
}

/// Run the task executor for one tick
///
/// This is just shorthand for:
/// ```no_run
/// # let _ = // avoid warnings from not capturing returned value
/// screeps_async::with_runtime(|runtime| {
///     runtime.run()
/// });
/// ```
///
/// # Panics
///
/// This function panics if the current runtime is not set
pub fn run() -> Result<(), RuntimeError> {
    with_runtime(|runtime| runtime.run())
}

/// The main entrypoint for the async runtime. Runs a future to completion.
///
/// See [ScreepsRuntime::block_on] for details
pub fn block_on<F>(future: F) -> Result<F::Output, RuntimeError>
where
    F: Future + 'static,
{
    with_runtime(|runtime| runtime.block_on(future))
}

/// Spawn a new async task
///
/// # Panics
///
/// This function panics if the current runtime is not set
pub fn spawn<F>(future: F) -> JobHandle<F::Output>
where
    F: Future + 'static,
{
    with_runtime(|runtime| runtime.spawn(future))
}

/// Acquire a reference to the [ScreepsRuntime].
///
/// # Panics
///
/// This function panics if the current runtime is not set
pub fn with_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&ScreepsRuntime) -> R,
{
    CURRENT.with_borrow(|runtime| {
        let runtime = runtime
            .as_ref()
            .expect("No screeps_async runtime configured");
        f(runtime)
    })
}

#[cfg(not(test))]
mod utils {
    use screeps::game;

    pub(super) fn game_time() -> u32 {
        game::time()
    }

    /// Returns the percentage of tick time used so far
    pub(super) fn time_used() -> f64 {
        game::cpu::get_used() / game::cpu::tick_limit()
    }
}

#[cfg(test)]
mod utils {
    pub(super) use super::tests::*;
}

#[cfg(test)]
mod tests {
    use crate::runtime::Builder;
    use std::cell::RefCell;

    thread_local! {
        pub(crate) static GAME_TIME: RefCell<u32> = RefCell::new(0);
        pub(crate) static TIME_USED: RefCell<f64> = RefCell::new(0.0);
    }

    pub(super) fn game_time() -> u32 {
        GAME_TIME.with_borrow(|t| *t)
    }

    pub(super) fn time_used() -> f64 {
        TIME_USED.with_borrow(|t| *t)
    }

    pub(crate) fn init_test() {
        GAME_TIME.with_borrow_mut(|t| *t = 0);
        TIME_USED.with_borrow_mut(|t| *t = 0.0);

        Builder::new().apply()
    }
}
