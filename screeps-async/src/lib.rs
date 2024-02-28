mod task;
pub mod macros;
pub mod runtime;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use crossbeam::channel;
use crate::task::Task;

use self::utils::*;

/// Spawn a new async task
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static
{
    CURRENT.with_borrow(|runtime| {
        Task::spawn(future, &runtime.as_ref().unwrap().sender);
    })
}

/// Sleeps for `[dur]` game ticks.
pub async fn delay(dur: u32) {
    struct Delay {
        when: u32,
        timer_index: usize,
    }

    impl Future for Delay {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if game_time() >= self.when {
                return Poll::Ready(())
            }

            CURRENT.with_borrow_mut(|runtime| {
                let mut timers = runtime.as_mut()
                    .expect("ScreepsRuntime not configured")
                    .timers
                    .try_lock()
                    .unwrap(); // attempting to lock twice will cause deadlock since screeps is single-threaded. Panic instead
                
                // SAFETY: timers map gets populated before future is created and removed on final wake
                let wakers = timers.get_mut(&self.when).unwrap();
                
                if let Some(waker) = wakers.get_mut(self.timer_index).and_then(Option::as_mut) {
                    // Waker already registered, check if it needs updating
                    if !waker.will_wake(cx.waker()) {
                        *waker = cx.waker().clone();
                    }
                } else {
                    // First time this future was polled, save the waker
                    wakers[self.timer_index] = Some(cx.waker().clone())
                }
            });

            Poll::Pending
        }
    }

    let when = game_time() + dur;
    let future = CURRENT.with_borrow_mut(|runtime| {
        let runtime = runtime.as_ref().unwrap();
        let mut timer_map = runtime.timers.try_lock().unwrap();
        let wakers = timer_map.entry(when).or_default();

        let timer_index = wakers.len();
        wakers.push(None); // Store an empty waker to ensure len() is incremented for the next delay
        Delay {
            when,
            timer_index
        }
    });

    // Wait for duration to complete
    future.await
}

/// Delay execution until the next tick
pub async fn yield_tick() {
    delay(1).await
}

struct ThreadLocalRuntime {
    sender: channel::Sender<Arc<Task>>,
    timers: Arc<Mutex<TimerMap>>
}

type TimerMap = BTreeMap<u32, Vec<Option<Waker>>>;

// Used to track the current mini-tokio instance so that the `spawn` function is
// able to schedule spawned tasks.
thread_local! {
    static CURRENT: RefCell<Option<ThreadLocalRuntime>> =
        const { RefCell::new(None) };
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
    use rstest::rstest;
    use std::sync::RwLock;
    use crate::runtime::ScreepsRuntime;
    use super::*;
    
    #[rstest]
    #[case(0, 0)]
    #[case(1, 1)]
    #[case(4, 4)]
    fn test_delay(#[case] dur: u32, #[case] expected: u32) {
        init_test();
        let has_run = Arc::new(Mutex::new(false));
        let has_run_clone = has_run.clone();

        let mut runtime = ScreepsRuntime::new(Default::default());
        
        spawn(async move {
            assert_eq!(0, game_time());
            delay(dur).await;
            assert_eq!(expected, game_time());

            let mut has_run = has_run_clone.lock().unwrap();
            *has_run = true;
        });

        // task hasn't run yet
        assert!(!*has_run.lock().unwrap());

        // Should complete within `dur` ticks (since we have infinite cpu time in this test)
        while game_time() <= dur {
            runtime.run();
            tick();
        }

        // Future has been run
        assert!(*has_run.lock().unwrap(), "Future failed to complete");
    }

    #[test]
    fn test_respects_time_remaining() {
        init_test();
        let has_run = Arc::new(Mutex::new(false));
        let has_run_clone = has_run.clone();

        let mut runtime = ScreepsRuntime::new(Default::default());
        *TIME_REMAINING.write().unwrap() = 0.05;

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
    
    fn tick() {
        *GAME_TIME.write().unwrap() += 1;
    }

    static GAME_TIME: RwLock<u32> = RwLock::new(0);
    pub(super) fn game_time() -> u32 {
        *GAME_TIME.read().unwrap()
    }

    static TIME_REMAINING: RwLock<f64> = RwLock::new(1.0);
    pub(super) fn time_used() -> f64 {
        *TIME_REMAINING.read().unwrap()
    }

    fn init_test() {
        *GAME_TIME.write().unwrap() = 0;
        *TIME_REMAINING.write().unwrap() = 1.0;
    }
}