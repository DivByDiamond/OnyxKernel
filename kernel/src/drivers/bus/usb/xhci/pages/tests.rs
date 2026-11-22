//! Ledger tests: allocation failure, overflow, partial unwind and the
//! scratch-page guard all leave the allocator balanced.

use super::*;
use core::cell::Cell;

/// Counting fake page allocator: hands out 0x1000-stepped addresses,
/// optionally failing from the `fail_at`-th allocation on.
struct Fake {
    next: Cell<u64>,
    allocs: Cell<usize>,
    frees: Cell<usize>,
    fail_at: usize,
}

impl Fake {
    fn new(fail_at: usize) -> Self {
        Fake {
            next: Cell::new(0x1000),
            allocs: Cell::new(0),
            frees: Cell::new(0),
            fail_at,
        }
    }
    fn alloc(&self) -> KResult<u64> {
        if self.allocs.get() >= self.fail_at {
            return Err(Errno::NoMem);
        }
        self.allocs.set(self.allocs.get() + 1);
        let pa = self.next.get();
        self.next.set(pa + 0x1000);
        Ok(pa)
    }
    fn free(&self, _pa: u64) {
        self.frees.set(self.frees.get() + 1);
    }
    fn live(&self) -> usize {
        self.allocs.get() - self.frees.get()
    }
}

#[test]
fn take_records_and_release_all_frees_every_page() {
    let f = Fake::new(usize::MAX);
    let mut set = PageSet::new();
    for _ in 0..3 {
        set.take(|| f.alloc(), |pa| f.free(pa)).unwrap();
    }
    assert_eq!(set.len(), 3);
    set.release_all(|pa| f.free(pa));
    assert_eq!(set.len(), 0);
    assert_eq!(f.live(), 0);
}

#[test]
fn failed_alloc_records_nothing_and_unwinds_to_the_checkpoint() {
    let f = Fake::new(3);
    let mut set = PageSet::new();
    set.take(|| f.alloc(), |pa| f.free(pa)).unwrap(); // persistent page
    let mark = set.len();
    set.take(|| f.alloc(), |pa| f.free(pa)).unwrap();
    set.take(|| f.alloc(), |pa| f.free(pa)).unwrap();
    // Third step of the multi-page arm fails: unwind just that step.
    assert_eq!(set.take(|| f.alloc(), |pa| f.free(pa)), Err(Errno::NoMem));
    set.release_from(mark, |pa| f.free(pa));
    assert_eq!(set.len(), 1);
    assert_eq!(f.live(), 1);
    set.release_all(|pa| f.free(pa));
    assert_eq!(f.live(), 0);
}

#[test]
fn full_ledger_frees_the_fresh_page_instead_of_leaking_it() {
    let f = Fake::new(usize::MAX);
    let mut set = PageSet::new();
    for _ in 0..DEV_PAGES {
        set.take(|| f.alloc(), |pa| f.free(pa)).unwrap();
    }
    assert_eq!(set.take(|| f.alloc(), |pa| f.free(pa)), Err(Errno::NoMem));
    assert_eq!(f.live(), DEV_PAGES);
    set.release_all(|pa| f.free(pa));
    assert_eq!(f.live(), 0);
}

#[test]
fn scratch_page_is_freed_on_success_and_on_error() {
    let f = Fake::new(usize::MAX);
    let ok: KResult<u32> = with_scratch_page(|| f.alloc(), |pa| f.free(pa), |_| Ok(7));
    assert_eq!(ok, Ok(7));
    assert_eq!(f.live(), 0);
    let bad: KResult<u32> = with_scratch_page(|| f.alloc(), |pa| f.free(pa), |_| Err(Errno::Io));
    assert_eq!(bad, Err(Errno::Io));
    assert_eq!(f.live(), 0);
    // Allocation failure: the closure never runs, nothing to free.
    let none = Fake::new(0);
    let r: KResult<u32> = with_scratch_page(|| none.alloc(), |pa| none.free(pa), |_| Ok(1));
    assert_eq!(r, Err(Errno::NoMem));
    assert_eq!(none.live(), 0);
}
