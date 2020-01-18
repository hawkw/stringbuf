pub(crate) mod loom;

use crate::loom::{
    atomic::{AtomicUsize, Ordering},
    CausalCell,
};

#[derive(Clone)]
pub struct Writer {
    inner: Arc<Inner>,
}

pub struct Reader {
    inner: Weak<Inner>,
    next: usize,
}

struct Inner {
    r_i: AtomicUsize,
    w_i: AtomicUsize,
    len: usize,
    buf: [CausalCell<String>],
}

impl Writer {
    pub fn write<T>(&self, f: impl FnOnce(&mut String) -> T) -> T {
        let this = *self.inner;
        // XXX(eliza): there is maybe a bug here if writes on other threads
        // "lap" us while we are still writing...tbqh, we could protect against
        // this w/ a mutex...
        let idx = this.w_i.fetch_add(1, Ordering::AcqRel);
        // we now exclusively own `idx`
        let res = this.buf[idx % this.len].with_mut(|s| {
            let s = unsafe { &mut *s };
            s.clear();
            f(s)
        });
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
    pub fn read<T>(&mut self, f: impl FnOnce(&String) -> T) -> Result<Option<T>, Closed> {
        let this = Arc::upgrade(self.inner).ok_or(Closed { _p: () })?;
        let read_ix = this.r_i.load(Ordering::Acquire);
        if self.next >= read_ix {
            // gotta slow down!
            return Ok(None);
        }

        let res = this.buf[self.next % this.len].with(|s| {
            let s = unsafe { &*s };
            f(s)
        });

        self.next += 1;
        Ok(Some(res))
    }
}
