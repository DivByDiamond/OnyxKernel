use super::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    fail_alloc: bool,
    allocated: usize,
    roots: Vec<u64>,
    freed: usize,
}
struct Memory(Rc<RefCell<State>>);
impl ExecMemory for Memory {
    fn allocate_refcount(&mut self) -> KResult<*mut u32> {
        let mut state = self.0.borrow_mut();
        if state.fail_alloc {
            return Err(Errno::NoMem);
        }
        state.allocated += 1;
        Ok(Box::into_raw(Box::new(1)))
    }
    fn destroy_root(&mut self, root: u64) {
        self.0.borrow_mut().roots.push(root);
    }
    fn free_refcount(&mut self, rc: *mut u32) {
        self.0.borrow_mut().freed += 1;
        unsafe {
            drop(Box::from_raw(rc));
        }
    }
}
fn image(ring: u8) -> OnxLoadResult {
    OnxLoadResult {
        entry: 0x10000,
        root_pa: 0x123000,
        ustack: 0x40000000,
        heap_brk: 0x1000000,
        ring,
    }
}

#[test]
fn denied_promotion_releases_loaded_root_without_allocating() {
    let state = Rc::new(RefCell::new(State::default()));
    let result = PreparedExec::with_memory(image(1), 2, Memory(state.clone()));
    assert!(matches!(result, Err(Errno::Perm)));
    let state = state.borrow();
    assert_eq!(state.roots, [0x123000]);
    assert_eq!(state.allocated, 0);
}

#[test]
fn refcount_allocation_failure_releases_new_root() {
    let state = Rc::new(RefCell::new(State {
        fail_alloc: true,
        ..State::default()
    }));
    let result = PreparedExec::with_memory(image(2), 2, Memory(state.clone()));
    assert!(matches!(result, Err(Errno::NoMem)));
    assert_eq!(state.borrow().roots, [0x123000]);
}

#[test]
fn abandoned_preparation_releases_root_and_refcount() {
    let state = Rc::new(RefCell::new(State::default()));
    drop(PreparedExec::with_memory(image(2), 2, Memory(state.clone())).unwrap());
    let state = state.borrow();
    assert_eq!(state.roots, [0x123000]);
    assert_eq!(state.freed, 1);
}

#[test]
fn commit_transfers_ownership_without_releasing_resources() {
    for (caller, target) in [(1, 1), (1, 2), (2, 2)] {
        let state = Rc::new(RefCell::new(State::default()));
        let prepared =
            PreparedExec::with_memory(image(target), caller, Memory(state.clone())).unwrap();
        let (image, rc) = prepared.commit();
        assert_eq!(image.root_pa, 0x123000);
        assert_eq!(unsafe { *rc }, 1);
        assert!(state.borrow().roots.is_empty());
        assert_eq!(state.borrow().freed, 0);
        unsafe {
            drop(Box::from_raw(rc));
        }
    }
}

#[test]
fn shared_root_has_exactly_one_last_owner_under_concurrent_release() {
    use std::sync::{Arc, Barrier, atomic::AtomicU32};
    let refs = Arc::new(AtomicU32::new(2));
    let start = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let refs = refs.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                // SAFETY: Arc keeps this aligned atomic cell live for both drops.
                unsafe { crate::proc::dec_root_refcount(Arc::as_ptr(&refs).cast_mut().cast()) }
            })
        })
        .collect();
    let last_owners = workers
        .into_iter()
        .map(|worker| worker.join().unwrap() as usize)
        .sum::<usize>();
    assert_eq!(last_owners, 1);
    assert_eq!(refs.load(core::sync::atomic::Ordering::Acquire), 0);
}
