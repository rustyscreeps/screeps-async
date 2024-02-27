use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::Context;
use crossbeam::channel;
use futures::future::BoxFuture;
use futures::task;
use futures::task::ArcWake;

// Task harness. Contains the future as well as the necessary data to schedule
// the future once it is woken.
pub(crate) struct Task {
    // BoxFuture<T> is a type alias for:
    // Pin<Box<dyn Future<Output = T> + Send + 'static>>
    future: Mutex<BoxFuture<'static, ()>>,

    // When a task is notified, it is queued into this channel. The executor
    // pops notified tasks and executes them.
    executor: channel::Sender<Arc<Task>>,
}

impl Task {
    pub(crate) fn spawn<F>(future: F, sender: &channel::Sender<Arc<Task>>)
        where
            F: Future<Output = ()> + Send + 'static,
    {
        let task = Arc::new(Task {
            future: Mutex::new(Box::pin(future)),
            executor: sender.clone(),
        });

        let _ = sender.send(task);
    }

    // Execute a scheduled task. This creates the necessary `task::Context`
    // containing a waker for the task. This waker pushes the task onto the
    // scheduled channel. The future is then polled with the waker.
    pub(crate) fn poll(self: Arc<Self>) {
        let waker = task::waker(self.clone());
        let mut cx = Context::from_waker(&waker);

        let mut future = self.future.try_lock().unwrap();

        let _ = future.as_mut().poll(&mut cx);
    }
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let _ = arc_self.executor.send(arc_self.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_spawn() {
        let future = async {};
        
        spawn_future(future);
    }
    
    #[test]
    fn test_poll() {
        let has_run = Arc::new(Mutex::new(false));
        let has_run_clone = has_run.clone();
        let task = spawn_future(async move {
            let mut has_run = has_run_clone.lock().unwrap();
            *has_run = true;
        });
        
        // task hasn't run yet
        assert!(!*has_run.lock().unwrap());
        
        task.poll();
        
        // Future has been run
        assert!(*has_run.lock().unwrap());
    }
    
    fn spawn_future<F>(future: F) -> Arc<Task>
    where
        F: Future<Output=()> + Send + 'static
    {
        let (send, recv) = channel::unbounded();
        Task::spawn(future, &send);

        recv.try_recv().expect("No task sent to queue")
    }
}