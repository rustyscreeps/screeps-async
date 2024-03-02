//! See [JobHandle]

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

/// Reference to a [Future] that has been scheduled via [send](crate::ScreepsRuntime::spawn)
///
/// This type is safe to drop without canceling the underlying task
///
/// This type implements [Future] to allow awaiting on the result of the spawned task
pub struct JobHandle<T> {
    fut_res: Rc<RefCell<Option<T>>>,
    complete: bool,
}

impl<T> JobHandle<T> {
    pub(crate) fn new(fut_res: Rc<RefCell<Option<T>>>) -> Self {
        Self {
            fut_res,
            complete: false,
        }
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
