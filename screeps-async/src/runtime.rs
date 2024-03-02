//! The Screeps Async runtime

use crate::error::RuntimeError;
use crate::job::JobHandle;
use crate::utils::{game_time, time_used};
use crate::CURRENT;
use async_task::Runnable;
use std::cell::RefCell;
use std::collections::btree_map::Entry;
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
            sender.send(runnable).unwrap();
        });

        task.detach(); // Ensure this task can run in the background
        runnable.schedule();

        JobHandle::new(fut_res)
    }

    /// Run the executor for one game tick
    ///
    /// This should generally be the last thing you call in your loop as by default the runtime
    /// will keep polling for work until 90% of this tick's CPU time has been exhausted.
    /// Thus, with enough scheduled work, this function will run for AT LEAST 90% of the tick time
    /// (90% + however long the last Future takes to poll)
    pub fn run(&self) -> Result<(), RuntimeError> {
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

        // Poll for tasks as long as we have time left this tick
        while time_used() <= self.config.tick_time_allocation {
            if let Ok(runnable) = self.scheduled.try_recv() {
                runnable.run();
            } else {
                // No more tasks scheduled this tick, quit polling for more
                return Ok(());
            }
        }

        // we ran out of time :(
        Err(RuntimeError::OutOfTime)
    }
}

type TimerMap = BTreeMap<u32, Vec<Option<Waker>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RuntimeError::OutOfTime;
    use crate::tests::*;
    use crate::{spawn, with_runtime};
    use std::cell::OnceCell;

    #[test]
    fn test_spawn() {
        init_test();

        let _ = spawn(async move {});

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
            });
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

        spawn(async move {
            let result = spawn(async move { 1 + 2 }).await;

            assert_eq!(3, result);
        });

        crate::run().unwrap();
    }

    #[test]
    fn test_respects_time_remaining() {
        init_test();

        let has_run = Rc::new(OnceCell::new());
        {
            let has_run = has_run.clone();
            spawn(async move {
                has_run.set(()).unwrap();
            });
        }

        TIME_USED.with_borrow_mut(|t| *t = 0.95);

        // task hasn't run yet
        assert!(has_run.get().is_none());

        assert_eq!(Err(OutOfTime), crate::run());

        // Check future still hasn't run
        assert!(has_run.get().is_none());
    }
}
