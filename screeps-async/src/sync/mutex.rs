use std::cell::{Cell, UnsafeCell};
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

/// An async mutex
///
/// Locks will be acquired in the order they are requested
///
/// # Examples
/// ```
/// # use std::rc::Rc;
/// # use screeps_async::sync::Mutex;
/// # screeps_async::initialize();
/// let mutex = Rc::new(Mutex::new(0));
/// screeps_async::spawn(async move {
///     let mut val = mutex.lock().await;
///     *val = 1;
/// }).detach();
/// ```
pub struct Mutex<T> {
    /// Whether the mutex is currently locked.
    ///
    /// Use [`Cell<bool>`] instead of [AtomicBool] since we don't really need atomics
    /// and [Cell] is more general
    state: Cell<bool>,
    /// Wrapped value
    data: UnsafeCell<T>,
    /// Queue of futures to wake when a lock is released
    wakers: UnsafeCell<Vec<Waker>>,
}

impl<T> Mutex<T> {
    /// Construct a new [Mutex] in the unlocked state wrapping the given value
    pub fn new(val: T) -> Self {
        Self {
            state: Cell::new(false),
            data: UnsafeCell::new(val),
            wakers: UnsafeCell::new(Vec::new()),
        }
    }

    /// Acquire the mutex.
    ///
    /// Returns a guard that release the mutex when dropped
    pub fn lock(&self) -> MutexLockFuture<'_, T> {
        MutexLockFuture::new(self)
    }

    /// Try to acquire the mutex.
    ///
    /// If the mutex could not be acquired at this time return [`None`], otherwise
    /// returns a guard that will release the mutex when dropped.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        (!self.state.replace(true)).then(|| MutexGuard::new(self))
    }

    /// Consumes the mutex, returning the underlying data
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    fn unlock(&self) {
        self.state.set(false);
        let wakers = unsafe { &mut *self.wakers.get() };
        wakers.drain(..).for_each(Waker::wake);
    }
}

/// An RAII guard that releases the mutex when dropped
pub struct MutexGuard<'a, T> {
    lock: &'a Mutex<T>,
}

impl<'a, T> MutexGuard<'a, T> {
    fn new(lock: &'a Mutex<T>) -> Self {
        Self { lock }
    }

    /// Immediately drops the guard, and consequently unlocks the mutex.
    ///
    /// This function is equivalent to calling [`drop`] on the guard but is more self-documenting.
    pub fn unlock(self) {
        drop(self);
    }

    /// Release the lock and immediately yield control back to the async runtime
    ///
    /// This essentially just calls [Self::unlock] then [yield_now()](crate::time::yield_now)
    pub async fn unlock_fair(self) {
        self.unlock();
        crate::time::yield_now().await;
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}

/// A [Future] that blocks until the [Mutex] can be locked, then returns the [MutexGuard]
pub struct MutexLockFuture<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<'a, T> MutexLockFuture<'a, T> {
    fn new(mutex: &'a Mutex<T>) -> Self {
        Self { mutex }
    }
}

impl<'a, T> Future for MutexLockFuture<'a, T> {
    type Output = MutexGuard<'a, T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(val) = self.mutex.try_lock() {
            return Poll::Ready(val);
        }

        unsafe {
            (*self.mutex.wakers.get()).push(cx.waker().clone());
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::time::delay_ticks;
    use std::rc::Rc;

    #[test]
    fn single_lock() {
        crate::tests::init_test();

        let mutex = Rc::new(Mutex::new(vec![]));
        {
            let mutex = mutex.clone();
            crate::spawn(async move {
                let mut vec = mutex.lock().await;
                vec.push(0);
            })
            .detach();
        }

        crate::run().unwrap();

        let expected = vec![0];
        let actual = Rc::into_inner(mutex).unwrap().into_inner();
        assert_eq!(expected, actual);
    }

    #[test]
    fn cannot_lock_twice() {
        let mutex = Mutex::new(());
        let _guard = mutex.try_lock().unwrap();

        assert!(mutex.try_lock().is_none());
    }

    #[test]
    fn await_multiple_locks() {
        crate::tests::init_test();

        let mutex = Rc::new(Mutex::new(vec![]));
        const N: u32 = 10;
        for i in 0..N {
            let mutex = mutex.clone();
            crate::spawn(async move {
                let mut vec = mutex.lock().await;
                // Release the lock next tick to guarantee blocked tasks
                delay_ticks(1).await;
                vec.push(i);
            })
            .detach();
        }

        for _ in 0..=N {
            crate::tests::tick().unwrap();
        }

        let expected = (0..10).collect::<Vec<_>>();
        let actual = Rc::into_inner(mutex).unwrap().into_inner();
        assert_eq!(expected, actual);
    }

    #[test]
    fn handles_dropped_futures() {
        crate::tests::init_test();

        let mutex = Rc::new(Mutex::new(vec![]));
        {
            let mutex = mutex.clone();
            crate::spawn(async move {
                let mut _guard = mutex.lock().await;
                delay_ticks(1).await;
                _guard.push(0);
            })
            .detach();
        }
        let to_drop = {
            let mutex = mutex.clone();
            crate::spawn(async move {
                let mut _guard = mutex.lock().await;
                _guard.push(1);
            })
        };
        {
            let mutex = mutex.clone();
            crate::spawn(async move {
                let mut _guard = mutex.lock().await;
                _guard.push(2);
            })
            .detach();
        }

        crate::tests::tick().unwrap();
        drop(to_drop);
        crate::tests::tick().unwrap();

        let expected = vec![0, 2];
        let actual = Rc::into_inner(mutex).unwrap().into_inner();

        assert_eq!(expected, actual);
    }
}
