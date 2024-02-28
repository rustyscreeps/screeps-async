//! Utilities for tracking time

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use crate::runtime::CURRENT;
use crate::utils::game_time;

/// Future returned by [delay]
pub struct Delay {
    when: u32,
    timer_index: usize,
}

impl Delay {
    fn new(when: u32) -> Self {
        CURRENT.with_borrow_mut(|runtime| {
            let runtime = runtime.as_ref().unwrap();
            let mut timer_map = runtime.timers.try_lock().unwrap();
            let wakers = timer_map.entry(when).or_default();

            let timer_index = wakers.len();
            wakers.push(None); // Store an empty waker to ensure len() is incremented for the next delay
            Delay {
                when,
                timer_index
            }
        })
    }
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

/// Sleeps for `[dur]` game ticks.
pub fn delay(dur: u32) -> Delay {
    let when = game_time() + dur;
    Delay::new(when)
}

/// Sleep until [screeps::game::time()] >= `when`
pub fn delay_until(when: u32) -> Delay {
    Delay::new(when)
}

/// Delay execution until the next tick
pub async fn yield_tick() {
    delay(1).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use rstest::rstest;
    use crate::runtime::ScreepsRuntime;
    use crate::spawn;
    use crate::tests::game_time;

    #[rstest]
    #[case(0, 0)]
    #[case(1, 1)]
    #[case(4, 4)]
    fn test_delay(#[case] dur: u32, #[case] expected: u32) {
        crate::tests::init_test();
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
            crate::tests::GAME_TIME.with_borrow_mut(|t| *t = *t + 1);
        }

        // Future has been run
        assert!(*has_run.lock().unwrap(), "Future failed to complete");
    }
}