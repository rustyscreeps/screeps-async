use std::cell::{Ref, RefCell, RefMut, UnsafeCell};
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

/// An async RwLock
///
/// Locks will be acquired in the order they are requested. When any task is waiting
/// on a [write](RwLock::write) lock, no new [read](RwLock::read) locks can be acquired
pub struct RwLock<T> {
    /// Inner RwLock
    inner: RefCell<T>,
    /// Queue of futures to wake when a write lock is released
    read_wakers: UnsafeCell<Vec<Waker>>,
    /// Queue of futures to wake when a read lock is released
    write_wakers: UnsafeCell<Vec<Waker>>,
}

impl<T> RwLock<T> {
    /// Construct a new [RwLock] wrapping `val`
    pub fn new(val: T) -> Self {
        Self {
            inner: RefCell::new(val),
            read_wakers: UnsafeCell::new(Vec::new()),
            write_wakers: UnsafeCell::new(Vec::new()),
        }
    }

    /// Block until the wrapped value can be immutably borrowed
    pub fn read(&self) -> RwLockFuture<'_, T, RwLockReadGuard<'_, T>> {
        RwLockFuture {
            lock: self,
            borrow: Self::try_read,
            is_writer: false,
        }
    }

    /// Attempt to immutably borrow the wrapped value.
    ///
    /// Returns [None] if the value is currently mutably borrowed or
    /// a task is waiting on a mutable reference.
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        unsafe { RwLockReadGuard::new(self) }
    }

    /// Block until the wrapped value can be mutably borrowed
    pub fn write(&self) -> RwLockFuture<'_, T, RwLockWriteGuard<'_, T>> {
        RwLockFuture {
            lock: self,
            borrow: Self::try_write,
            is_writer: true,
        }
    }

    /// Attempt to mutably borrow the wrapped value.
    ///
    /// Returns [None] if the value is already borrowed (mutably or immutably)
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        RwLockWriteGuard::new(self)
    }

    /// Consumes this [RwLock] and returns ownership of the wrapped value
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// Convenience method to consume [`Rc<RwLock<T>>`] and return the wrapped value
    ///
    /// # Panics
    /// This method panics if the Rc has more than one strong reference
    pub fn into_inner_rc(self: Rc<Self>) -> T {
        Rc::into_inner(self).unwrap().into_inner()
    }
}

impl<T> RwLock<T> {
    unsafe fn unlock(&self) {
        let wakers = &mut *self.write_wakers.get();
        wakers.drain(..).for_each(Waker::wake);

        let wakers = &mut *self.read_wakers.get();
        wakers.drain(..).for_each(Waker::wake);
    }
}

/// An RAII guard that releases a read lock when dropped
pub struct RwLockReadGuard<'a, T> {
    inner: &'a RwLock<T>,
    data: Ref<'a, T>,
}

impl<'a, T> RwLockReadGuard<'a, T> {
    unsafe fn new(lock: &'a RwLock<T>) -> Option<Self> {
        if !(*lock.write_wakers.get()).is_empty() {
            return None; // Cannot take new reads if a writer is waiting
        }

        let data = lock.inner.try_borrow().ok()?;

        Some(RwLockReadGuard { data, inner: lock })
    }
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.inner.unlock() }
    }
}

impl<T> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// An RAII guard that releases the write lock when dropped
pub struct RwLockWriteGuard<'a, T> {
    inner: &'a RwLock<T>,
    data: RefMut<'a, T>,
}

impl<'a, T> RwLockWriteGuard<'a, T> {
    fn new(lock: &'a RwLock<T>) -> Option<Self> {
        let data = lock.inner.try_borrow_mut().ok()?;

        Some(Self { inner: lock, data })
    }

    /// Immediately drop the guard and release the write lock
    ///
    /// Equivalent to [drop(self)], but is more self-documenting
    pub fn unlock(self) {
        drop(self);
    }

    /// Release the write lock and immediately yield control back to the async runtime
    ///
    /// This essentially just calls [Self::unlock] then [yield_now()](crate::time::yield_now)
    pub async fn unlock_fair(self) {
        self.unlock();
        crate::time::yield_now().await;
    }
}

impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.inner.unlock() }
    }
}

impl<T> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

/// A [Future] that blocks until the [RwLock] can be acquired.
pub struct RwLockFuture<'a, T, G> {
    lock: &'a RwLock<T>,
    borrow: fn(&'a RwLock<T>) -> Option<G>,
    is_writer: bool,
}

impl<T, G> Future for RwLockFuture<'_, T, G> {
    type Output = G;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(guard) = (self.borrow)(self.lock) {
            return Poll::Ready(guard);
        }

        let wakers = if self.is_writer {
            self.lock.write_wakers.get()
        } else {
            self.lock.read_wakers.get()
        };
        let wakers = unsafe { &mut *wakers };

        wakers.push(cx.waker().clone());

        Poll::Pending
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::time::delay_ticks;

    #[test]
    fn can_read_multiple_times() {
        crate::tests::init_test();

        let lock = Rc::new(RwLock::new(()));
        const N: usize = 10;
        for _ in 0..N {
            let lock = lock.clone();
            crate::spawn(async move {
                let _guard = lock.read().await;
                // Lock should acquire first tick
                assert_eq!(0, crate::tests::game_time());
                // don't release till next tick to check if we can hold multiple read locks at once
                delay_ticks(1).await;
            })
            .detach();
        }

        for _ in 0..=N {
            crate::tests::tick().unwrap();
        }
    }

    #[test]
    fn cannot_write_multiple_times() {
        crate::tests::init_test();

        let lock = Rc::new(RwLock::new(0));
        {
            let lock = lock.clone();
            crate::spawn(async move {
                let mut guard = lock.write().await;
                assert_eq!(0, crate::tests::game_time());
                delay_ticks(1).await;
                *guard += 1;
            })
            .detach();
        }
        {
            let lock = lock.clone();
            crate::spawn(async move {
                let mut guard = lock.write().await;
                assert_eq!(1, crate::tests::game_time());
                delay_ticks(1).await;
                *guard += 1;
            })
            .detach();
        }

        crate::tests::tick().unwrap();
        crate::tests::tick().unwrap();
        crate::tests::tick().unwrap();

        assert_eq!(2, lock.into_inner_rc());
    }

    #[test]
    fn cannot_read_while_writer_waiting() {
        crate::tests::init_test();

        let lock = Rc::new(RwLock::new(0));
        {
            let lock = lock.clone();
            crate::spawn(async move {
                let mut guard = lock.write().await;
                println!("write 1 acquired");
                assert_eq!(0, crate::tests::game_time());
                delay_ticks(1).await;
                *guard += 1;
            })
            .detach();
        }
        {
            let lock = lock.clone();
            crate::spawn(async move {
                let guard = lock.read().await;
                println!("read 1 acquired");
                // this should happen after second write
                assert_eq!(2, crate::tests::game_time());
                delay_ticks(1).await;
                assert_eq!(2, *guard);
            })
            .detach();
        }
        {
            let lock = lock.clone();
            crate::spawn(async move {
                let mut guard = lock.write().await;
                println!("write 2 acquired");
                assert_eq!(1, crate::tests::game_time());
                delay_ticks(1).await;
                *guard += 1;
            })
            .detach();
        }
        {
            let lock = lock.clone();
            crate::spawn(async move {
                let guard = lock.read().await;
                println!("read 2 acquired");
                assert_eq!(2, crate::tests::game_time());
                assert_eq!(2, *guard);
            })
            .detach();
        }

        crate::tests::tick().unwrap();
        crate::tests::tick().unwrap();
        crate::tests::tick().unwrap();
        crate::tests::tick().unwrap();

        assert_eq!(2, lock.into_inner_rc());
    }
}
