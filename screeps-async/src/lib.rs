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
//! use std::cell::RefCell;
//! use std::rc::Rc;
//! use screeps_async::runtime::ScreepsRuntime;
//! // Use thread local just as a convenient way to avoid Send/Sync requirements as Screeps is single-threaded
//! // You may store the runtime however you like as long as it persists across ticks
//! thread_local! {
//!     static RUNTIME: RefCell<ScreepsRuntime> = RefCell::new(screeps_async::runtime::Builder::new().build());
//! }
//!
//! pub fn game_loop() {
//!     // Tick logic that spawns some tasks
//!     screeps_async::spawn(async {
//!         println!("Hello!");
//!     });
//!
//!     RUNTIME.with_borrow_mut(|runtime| {
//!         runtime.run()
//!     });
//! }
//! ```

pub mod macros;
pub use macros::*;
pub mod runtime;
// mod task;
pub mod time;

use crate::runtime::CURRENT;
use std::future::Future;

/// Spawn a new async task
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    CURRENT.with_borrow(|runtime| {
        let sender = runtime
            .as_ref()
            .expect("No ScreepsRuntime configured")
            .sender
            .clone();

        let (runnable, task) = async_task::spawn_local(future, move |runnable| {
            sender.send(runnable).unwrap();
        });

        task.detach(); // TODO surface this to the user somehow instead of just detaching
        runnable.schedule();
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
    use crate::runtime::{Builder, ScreepsRuntime};
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

    pub(crate) fn init_test() -> ScreepsRuntime {
        GAME_TIME.with_borrow_mut(|t| *t = 0);
        TIME_USED.with_borrow_mut(|t| *t = 0.0);

        Builder::new().build()
    }
}
