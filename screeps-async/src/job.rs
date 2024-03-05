//! See [JobHandle]

use async_task::Task;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

/// Reference to a [Future] that has been scheduled via [send](crate::ScreepsRuntime::spawn)
///
/// Dropping a [`JobHandle`] cancels it, which means its future won't be polled again.
/// To drop the [`JobHandle`] handle without canceling it, use [detach()](JobHandle::detach) instead.
/// To cancel a task gracefully and wait until it is fully destroyed, use the [cancel()](JobHandle::cancel) method
///
/// This type implements [Future] to allow awaiting on the result of the spawned task
pub struct JobHandle<T> {
    pub(crate) fut_res: Rc<RefCell<Option<T>>>,
    task: Task<()>,
    complete: bool,
}

impl<T> JobHandle<T> {
    pub(crate) fn new(fut_res: Rc<RefCell<Option<T>>>, task: Task<()>) -> Self {
        Self {
            fut_res,
            task,
            complete: false,
        }
    }

    /// Cancels the task and waits for it to stop running.
    ///
    /// Returns the task's output if it was completed just before it got canceled, or [`None`] if
    /// it didn't complete.
    ///
    /// While it's possible to simply drop the [`JobHandle`] to cancel it, this is a cleaner way of
    /// canceling because it also waits for the task to stop running.
    pub async fn cancel(self) -> Option<T> {
        let _ = self.task.cancel().await;
        self.fut_res.take()
    }

    /// Detaches the task to let it keep running in the background.
    pub fn detach(self) {
        self.task.detach()
    }

    /// Check whether this job has completed
    pub fn is_complete(&self) -> bool {
        self.complete || self.fut_res.borrow().is_some()
    }
}

impl<T> Future for JobHandle<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.complete {
            panic!("Cannot await on a JobHandle that has already completed")
        }
        let res = self.fut_res.borrow_mut().take();

        match res {
            Some(res) => {
                self.complete = true;
                Poll::Ready(res)
            }
            None => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::spawn;
    use crate::tests::init_test;

    #[test]
    fn test_cancel() {
        init_test();

        let result = crate::block_on(async move {
            let task = spawn(async move { true });

            task.cancel().await
        })
        .unwrap();

        assert_eq!(None, result);
    }

    #[test]
    fn test_await() {
        init_test();

        let result = crate::block_on(async move { spawn(async move { true }).await }).unwrap();

        assert!(result, "Failed to await spawned future");
    }
}
