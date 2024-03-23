//! The Screeps Async runtime

use crate::error::RuntimeError;
use crate::job::JobHandle;
use crate::utils::{game_time, time_used};
use crate::CURRENT;
use async_task::Runnable;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::rc::Rc;
use std::sync::Mutex;
use std::task::Waker;

/// Builder to construct a [ScreepsRuntime]
pub struct Builder {
    config: Config,
}

impl Builder {
    /// Construct a new [Builder] with default settings
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Set what percentage of available CPU time the runtime should use per tick
    pub fn tick_time_allocation(mut self, dur: f64) -> Self {
        self.config.tick_time_allocation = dur;
        self
    }

    /// Build a [ScreepsRuntime]
    pub fn apply(self) {
        CURRENT.with_borrow_mut(|runtime| {
            *runtime = Some(ScreepsRuntime::new(self.config));
        })
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration options for the [ScreepsRuntime]
pub struct Config {
    /// Percentage of per-tick CPU time allowed to be used by the async runtime
    ///
    /// Specifically, the runtime will continue polling new futures as long as
    /// `[screeps::game::cpu::get_used] < tick_time_allocation * [screeps::game::cpu::tick_limit]`
    tick_time_allocation: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tick_time_allocation: 0.9,
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
    scheduled: flume::Receiver<Runnable>,

    /// Send half of the scheduled channel.
    sender: flume::Sender<Runnable>,

    /// Stores [`Waker`]s used to wake tasks that are waiting for a specific game tick
    // TODO should this really be pub(crate)?
    pub(crate) timers: Rc<Mutex<TimerMap>>,

    /// Config for the runtime
    config: Config,

    /// Mutex used to ensure you don't block_on multiple futures simultaneously
    is_blocking: Mutex<()>,
}

impl ScreepsRuntime {
    /// Initialize a new runtime instance.
    ///
    /// Only one ScreepsRuntime may exist. Attempting to create a second one before the first is
    /// dropped with panic
    pub(crate) fn new(config: Config) -> Self {
        let (sender, scheduled) = flume::unbounded();

        let timers = Rc::new(Mutex::new(BTreeMap::new()));

        Self {
            scheduled,
            sender,
            timers,
            config,
            is_blocking: Mutex::new(()),
        }
    }

    /// Spawn a new async task that will be polled next time the scheduler runs
    pub fn spawn<F>(&self, future: F) -> JobHandle<F::Output>
    where
        F: Future + 'static,
    {
        let fut_res = Rc::new(RefCell::new(None));

        let future = {
            let fut_res = fut_res.clone();
            async move {
                let res = future.await;
                let mut fut_res = fut_res.borrow_mut();
                *fut_res = Some(res);
            }
        };

        let sender = self.sender.clone();
        let (runnable, task) = async_task::spawn_local(future, move |runnable| {
            // Don't try to send if disconnected, this only happens when runtime is being dropped
            if !sender.is_disconnected() {
                sender.send(runnable).unwrap();
            }
        });

        runnable.schedule();

        JobHandle::new(fut_res, task)
    }

    /// The main entrypoint for the async runtime. Runs a future to completion.
    ///
    /// Returns [RuntimeError::DeadlockDetected] if blocking [Future] doesn't complete this tick
    ///
    /// # Panics
    ///
    /// Panics if another future is already being blocked on. You should `.await` the second future
    /// instead.
    pub fn block_on<F>(&self, future: F) -> Result<F::Output, RuntimeError>
    where
        F: Future + 'static,
    {
        let _guard = self
            .is_blocking
            .try_lock()
            .expect("Cannot block_on multiple futures at once. Please .await on the inner future");
        let handle = self.spawn(future);

        while !handle.is_complete() {
            if !self.try_poll_scheduled()? {
                return Err(RuntimeError::DeadlockDetected);
            }
        }

        Ok(handle.fut_res.take().unwrap())
    }

    /// Run the executor for one game tick
    ///
    /// This should generally be the last thing you call in your loop as by default the runtime
    /// will keep polling for work until 90% of this tick's CPU time has been exhausted.
    /// Thus, with enough scheduled work, this function will run for AT LEAST 90% of the tick time
    /// (90% + however long the last Future takes to poll)
    pub fn run(&self) -> Result<(), RuntimeError> {
        // Only need to call this once per tick since delay_ticks(0) will execute synchronously
        self.wake_timers();

        // Poll tasks until there are no more, or we get an error
        while self.try_poll_scheduled()? {}

        Ok(())
    }

    /// Attempts to poll the next scheduled task, ensuring that there is time left in the tick
    ///
    /// Returns [Ok(true)] if a task was successfully polled
    /// Returns [Ok(false)] if there are no tasks ready to poll
    /// Returns [Err] if we have run out of allocated time this tick
    pub(crate) fn try_poll_scheduled(&self) -> Result<bool, RuntimeError> {
        if time_used() > self.config.tick_time_allocation {
            return Err(RuntimeError::OutOfTime);
        }

        if let Ok(runnable) = self.scheduled.try_recv() {
            runnable.run();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn wake_timers(&self) {
        let game_time = game_time();
        let mut timers = self.timers.try_lock().unwrap();

        let to_fire = {
            // Grab timers that are still in the future. `timers` is now all timers that need firing
            let mut pending = timers.split_off(&(game_time + 1));
            // Switcheroo pending/timers so that `timers` is all timers that are scheduled in the future
            std::mem::swap(&mut pending, &mut timers);
            pending
        };

        to_fire
            .into_values()
            .flatten()
            .flatten()
            .for_each(Waker::wake);
    }
}

type TimerMap = BTreeMap<u32, Vec<Option<Waker>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RuntimeError::OutOfTime;
    use crate::tests::*;
    use crate::time::yield_now;
    use crate::{spawn, with_runtime};
    use std::cell::OnceCell;

    #[test]
    fn test_block_on() {
        init_test();

        let res = crate::block_on(async move {
            yield_now().await;
            1 + 2
        })
        .unwrap();

        assert_eq!(3, res);
    }

    #[test]
    fn test_spawn() {
        init_test();

        drop(spawn(async move {}));

        with_runtime(|runtime| {
            runtime
                .scheduled
                .try_recv()
                .expect("Failed to schedule task");
        })
    }

    #[test]
    fn test_run() {
        init_test();

        let has_run = Rc::new(OnceCell::new());
        {
            let has_run = has_run.clone();
            spawn(async move {
                has_run.set(()).unwrap();
            })
            .detach();
        }

        // task hasn't run yet
        assert!(has_run.get().is_none());

        crate::run().unwrap();

        // Future has been run
        assert!(has_run.get().is_some());
    }

    #[test]
    fn test_nested_spawn() {
        init_test();

        let has_run = Rc::new(OnceCell::new());
        {
            let has_run = has_run.clone();
            spawn(async move {
                let result = spawn(async move { 1 + 2 }).await;

                assert_eq!(3, result);

                has_run.set(()).unwrap();
            })
            .detach();
        }

        // task hasn't run yet
        assert!(has_run.get().is_none());

        crate::run().unwrap();

        // Future has been run
        assert!(has_run.get().is_some());
    }

    #[test]
    fn test_respects_time_remaining() {
        init_test();

        let has_run = Rc::new(OnceCell::new());
        {
            let has_run = has_run.clone();
            spawn(async move {
                has_run.set(()).unwrap();
            })
            .detach();
        }

        TIME_USED.with_borrow_mut(|t| *t = 0.95);

        // task hasn't run yet
        assert!(has_run.get().is_none());

        assert_eq!(Err(OutOfTime), crate::run());

        // Check future still hasn't run
        assert!(has_run.get().is_none());
    }
}
