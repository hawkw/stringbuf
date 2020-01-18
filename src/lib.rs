pub(crate) mod loom;

use crate::loom::{
    atomic::{self, AtomicUsize, Ordering},
    CausalCell, Mutex,
};
use std::sync::{Arc, Weak};

#[cfg(test)]
mod tests;

pub fn with_capacity(capacity: usize) -> (Writer, Reader) {
    let mut buf = Vec::with_capacity(capacity);
    buf.resize_with(capacity, || Mutex::new(String::new()));
    let buf = buf.into_boxed_slice();
    let inner = Arc::new(Inner {
        r_i: AtomicUsize::new(0),
        w_i: AtomicUsize::new(0),
        len: capacity,
        buf,
    });

    let reader = Reader {
        inner: inner.clone(),
        next: 0,
    };
    let writer = Writer { inner };

    (writer, reader)
}

#[derive(Clone)]
pub struct Writer {
    inner: Arc<Inner>,
}

pub struct Reader {
    inner: Arc<Inner>,
    next: usize,
}

struct Inner {
    r_i: AtomicUsize,
    w_i: AtomicUsize,
    len: usize,
    buf: Box<[Mutex<String>]>, // XXX(eliza): i hate the second box...
}

impl Writer {
    pub fn write<T>(&self, f: impl FnOnce(&mut String) -> T) -> T {
        let this = &*self.inner;
        // XXX(eliza): there is maybe a bug here if writes on other threads
        // "lap" us while we are still writing...tbqh, we could protect against
        // this w/ a mutex...
        let w = this.w_i.fetch_add(1, Ordering::Release);
        let idx = w % this.len;
        // we now exclusively own `idx` (unless someone laps us)...
        #[cfg(debug_assertions)]
        let mut lock = this.buf[idx]
            .try_lock()
            .expect("someone lapped us, slow the heck down!");
        #[cfg(not(debug_assertions))]
        let mut lock = this.buf[idx].lock().unwrap();

        let res = f(&mut *lock);
        // scootch read index
        this.r_i.fetch_add(1, Ordering::Release);
        res
    }
}

pub struct Closed {
    _p: (),
}

impl Reader {
    /// Returns `None`
    pub fn try_read<T>(&mut self, f: impl FnOnce(&String) -> T) -> Result<Option<T>, Closed> {
        let this = &*self.inner;
        let read_ix = this.r_i.load(Ordering::Acquire);
        if self.next >= read_ix {
            if Arc::strong_count(&self.inner) <= 1 {
                return Err(Closed { _p: () });
            } else {
                // gotta slow down!
                return Ok(None);
            }
        }
        let idx = self.next % this.len;

        #[cfg(debug_assertions)]
        let lock = this.buf[idx]
            .try_lock()
            .expect("unless poisoned, this should always succeed???");
        #[cfg(not(debug_assertions))]
        let lock = this.buf[idx].lock().unwrap();
        let res = f(&*lock);

        self.next += 1;
        Ok(Some(res))
    }

    pub fn read<T>(&mut self, f: impl FnOnce(&String) -> T) -> Result<T, Closed> {
        let this = &*self.inner;
        let mut read_ix;
        loop {
            read_ix = this.r_i.load(Ordering::Acquire);
            if self.next < read_ix {
                break;
            } else if Arc::strong_count(&self.inner) <= 1 {
                return Err(Closed { _p: () });
            }
            atomic::spin_loop_hint();
        }

        let idx = self.next % this.len;

        #[cfg(debug_assertions)]
        let lock = this.buf[idx]
            .try_lock()
            .expect("unless poisoned, this should always succeed???");
        #[cfg(not(debug_assertions))]
        let lock = this.buf[idx].lock().unwrap();
        let res = f(&*lock);

        self.next += 1;
        Ok(res)
    }
}
