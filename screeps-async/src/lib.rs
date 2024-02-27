mod task;

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

/// Configuration for the ScreepsRuntime
pub struct RuntimeConfig {
    /// Percentage of per-tick CPU time allowed to be used by the async runtime
    ///
    /// Specifically, the runtime will continue polling new futures as long as
    /// `[screeps::game::cpu::get_used] < tick_time_allocation * [screeps::game::cpu::tick_limit]`
    tick_time_allocation: f64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            tick_time_allocation: 0.1,
        }
    }
}

/// A very basic futures executor based on a channel. When tasks are woken, they
/// are scheduled by queuing them in the send half of the channel. The executor
/// waits on the receive half and executes received tasks.
///
/// When a task is executed, the send half of the channel is passed along via
/// the task's Waker.
pub struct ScreepsRuntime {
    /// Receives scheduled tasks. When a task is scheduled, the associated future
    /// is ready to make progress. This usually happens when a resource the task
    /// uses becomes ready to perform an operation.
    scheduled: channel::Receiver<Arc<Task>>,

    /// Send half of the scheduled channel.
    sender: channel::Sender<Arc<Task>>,

    /// Stores [`Waker`]s used to wake tasks that are waiting for a specific game tick
    timers: Arc<Mutex<TimerMap>>,

    /// Config for the runtime
    config: RuntimeConfig,
}

impl ScreepsRuntime {
    /// Initialize a new runtime instance
    pub fn new(config: RuntimeConfig) -> Self {
        let (sender, scheduled) = channel::unbounded();

        let timers = Arc::new(Mutex::new(BTreeMap::new()));

        CURRENT.with_borrow_mut(|runtime| {
            *runtime = Some(ThreadLocalRuntime {
                sender: sender.clone(),
                timers: timers.clone(),
            });
        });

        Self { scheduled, sender, timers, config }
    }

    /// Run the executor for one game tick
    pub fn run(&mut self) {
        CURRENT.with_borrow_mut(|sender| {
            *sender = Some(ThreadLocalRuntime {
                sender: self.sender.clone(),
                timers: self.timers.clone(),
            });
        });

        {
            let game_time = game_time();
            let mut timers = self.timers.try_lock().unwrap();
            // Find timers with triggers <= game_time
            let timers_to_fire = timers
                .keys()
                .cloned()
                .take_while(|&t| t <= game_time)
                .collect::<Vec<_>>();

            // Populate the execution channel with the timers that have triggered
            for timer in timers_to_fire.into_iter() {
                if let Entry::Occupied(entry) = timers.entry(timer) {
                    // remove the timer from the map and call the wakers
                    for waker in entry.remove().into_iter().flatten() {
                        waker.wake();
                    }
                }
            }
        }

        // Drain the scheduled tasks queue without blocking
        while let Ok(task) = self.scheduled.try_recv() {
            // First check if we have time left this tick
            if time_used() <= self.config.tick_time_allocation {
                break;
            }
            task.poll();
        }
    }
}

type TimerMap = BTreeMap<u32, Vec<Option<Waker>>>;

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