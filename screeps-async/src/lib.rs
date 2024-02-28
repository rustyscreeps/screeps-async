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
//! Create the runtime and ensure you call it once each tick:
//! ```
//! use std::cell::RefCell;
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
//!     // Should generally be the last thing in your loop as by default
//!     // the runtime will keep polling for work until 90% of this tick's
//!     // CPU time has been exhausted. Thus, with enough scheduled work,
//!     // this function will run for AT LEAST 90% of the tick time
//!     // (90% + however long the last Future takes to poll)
//!     RUNTIME.with_borrow_mut(|runtime| {
//!         runtime.run()
//!     });
//! }
//! ```


mod task;
pub mod macros;
pub mod runtime;
pub mod time;

use std::future::Future;
use crate::runtime::CURRENT;
use crate::task::Task;


/// Spawn a new async task
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static
{
    CURRENT.with_borrow(|runtime| {
        Task::spawn(future, &runtime.as_ref().unwrap().sender);
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
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};
    use crate::runtime::ScreepsRuntime;
    use super::*;

    #[test]
    fn test_respects_time_remaining() {
        init_test();
        let has_run = Arc::new(Mutex::new(false));
        let has_run_clone = has_run.clone();

        let mut runtime = ScreepsRuntime::new(Default::default());
        TIME_USED.with_borrow_mut(|t| *t = 0.95);

        spawn(async move {
            let mut has_run = has_run_clone.lock().unwrap();
            *has_run = true;
        });

        // task hasn't run yet
        assert!(!*has_run.lock().unwrap());

        runtime.run();

        // Check future still hasn't run
        assert!(!*has_run.lock().unwrap());
    }

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
    }
}