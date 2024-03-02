//! Utilities for tracking time

use crate::utils::game_time;
use crate::with_runtime;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Future returned by [delay_ticks]
pub struct Delay {
    when: u32,
    timer_index: usize,
}

impl Delay {
    fn new(when: u32) -> Self {
        with_runtime(|runtime| {
            let mut timer_map = runtime.timers.try_lock().unwrap();
            let wakers = timer_map.entry(when).or_default();

            let timer_index = wakers.len();
            wakers.push(None); // Store an empty waker to ensure len() is incremented for the next delay
            Delay { when, timer_index }
        })
    }
}

impl Future for Delay {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if game_time() >= self.when {
            return Poll::Ready(());
        }

        with_runtime(|runtime| {
            let mut timers = runtime.timers.try_lock().unwrap();

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
pub fn delay_ticks(dur: u32) -> Delay {
    let when = game_time() + dur;
    Delay::new(when)
}

/// Sleep until [screeps::game::time()] >= `when`
pub fn delay_until(when: u32) -> Delay {
    Delay::new(when)
}

/// Delay execution until the next tick
pub async fn yield_tick() {
    delay_ticks(1).await
}

/// Yields execution back to the async runtime, but doesn't necessarily wait until next tick
/// to continue execution.
///
/// Long-running tasks that perform a significant amount of synchronous work between `.await`s
/// can prevent other tasks from being executed. In the worst case, too much synchronous work in a row
/// can consume all remaining CPU time this tick since the scheduler cannot interrupt work in the middle
/// of synchronous sections of code. To alleviate this problem, [yield_now] should be called periodically
/// to yield control back to the scheduler and give other tasks a chance to run.
pub async fn yield_now() {
    delay_ticks(0).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn;
    use crate::tests::game_time;
    use rstest::rstest;
    use std::cell::OnceCell;
    use std::rc::Rc;

    #[rstest]
    #[case(0, 0)]
    #[case(1, 1)]
    #[case(4, 4)]
    fn test_delay_ticks(#[case] dur: u32, #[case] expected: u32) {
        crate::tests::init_test();

        let has_run = Rc::new(OnceCell::new());
        {
            let has_run = has_run.clone();

            spawn(async move {
                assert_eq!(0, game_time());
                delay_ticks(dur).await;
                assert_eq!(expected, game_time());

                has_run.set(()).unwrap();
            });
        }

        // task hasn't run yet
        assert!(has_run.get().is_none());

        // Should complete within `dur` ticks (since we have infinite cpu time in this test)
        while game_time() <= dur {
            crate::run().unwrap();
            crate::tests::GAME_TIME.with_borrow_mut(|t| *t += 1);
        }

        // Future has been run
        assert!(has_run.get().is_some(), "Future failed to complete");
    }
}
