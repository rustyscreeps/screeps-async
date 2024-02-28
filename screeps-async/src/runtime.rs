use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use crossbeam::channel;
use crate::{CURRENT, ThreadLocalRuntime, TimerMap};
use crate::task::Task;
use crate::utils::{game_time, time_used};


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

    pub fn tick_time_allocation(mut self, dur: f64) -> Self {
        self.config.tick_time_allocation = dur;
        self
    }

    /// Build a [ScreepsRuntime]
    pub fn build(self) -> ScreepsRuntime {
        ScreepsRuntime::new(self.config)
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
    scheduled: channel::Receiver<Arc<Task>>,

    /// Send half of the scheduled channel.
    sender: channel::Sender<Arc<Task>>,

    /// Stores [`Waker`]s used to wake tasks that are waiting for a specific game tick
    timers: Arc<Mutex<TimerMap>>,

    /// Config for the runtime
    config: Config
}

impl ScreepsRuntime {
    /// Initialize a new runtime instance
    pub fn new(config: Config) -> Self {
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
    ///
    /// This should generally be the last thing you call in your loop as by default the runtime
    /// will keep polling for work until 90% of this tick's CPU time has been exhausted.
    /// Thus, with enough scheduled work, this function will run for AT LEAST 90% of the tick time
    /// (90% + however long the last Future takes to poll)
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
