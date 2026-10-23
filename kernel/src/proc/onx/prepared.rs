//! Ownership of a replacement image until exec reaches its commit point.
use super::OnxLoadResult;
use crate::mm::{heap, vmm};
use onyx_core::errno::{Errno, KResult};

pub(crate) trait ExecMemory {
    fn allocate_refcount(&mut self) -> KResult<*mut u32>;
    fn destroy_root(&mut self, root: u64);
    fn free_refcount(&mut self, rc: *mut u32);
}

pub(crate) struct KernelMemory {
    _private: (),
}
impl ExecMemory for KernelMemory {
    fn allocate_refcount(&mut self) -> KResult<*mut u32> {
        // SAFETY: PreparedExec::prepare requires kernel context and SIE clear.
        unsafe {
            let rc = heap::kmalloc(core::mem::size_of::<u32>())? as *mut u32;
            *rc = 1;
            Ok(rc)
        }
    }
    fn destroy_root(&mut self, root: u64) {
        // SAFETY: the guard exclusively owns this freshly loaded root.
        unsafe { vmm::destroy_root(root) }
    }
    fn free_refcount(&mut self, rc: *mut u32) {
        // SAFETY: the guard owns the allocation returned above.
        unsafe { heap::kfree(rc.cast()) }
    }
}

pub(crate) struct PreparedExec<M: ExecMemory = KernelMemory> {
    image: Option<OnxLoadResult>,
    refcount: *mut u32,
    memory: M,
}

impl PreparedExec {
    /// # Safety
    /// `image` must be a newly loaded, exclusively owned address space.
    /// Call in kernel context with interrupts disabled; ownership transfers
    /// to this function even when permission or allocation checks fail.
    pub(crate) unsafe fn prepare(image: OnxLoadResult, ring: u8) -> KResult<Self> {
        Self::with_memory(image, ring, KernelMemory { _private: () })
    }
}

impl<M: ExecMemory> PreparedExec<M> {
    fn with_memory(image: OnxLoadResult, ring: u8, memory: M) -> KResult<Self> {
        let mut prepared = Self {
            image: Some(image),
            refcount: core::ptr::null_mut(),
            memory,
        };
        if ring == crate::proc::PROC_RING_USER
            && prepared.image().ring == crate::proc::PROC_RING_ROOT
        {
            return Err(Errno::Perm);
        }
        prepared.refcount = prepared.memory.allocate_refcount()?;
        Ok(prepared)
    }

    pub(crate) fn image(&self) -> &OnxLoadResult {
        self.image.as_ref().expect("uncommitted exec image")
    }

    /// Transfer both resources to the process; all fallible work must precede this.
    pub(crate) fn commit(mut self) -> (OnxLoadResult, *mut u32) {
        let image = self.image.take().expect("uncommitted exec image");
        let refcount = core::mem::replace(&mut self.refcount, core::ptr::null_mut());
        (image, refcount)
    }
}

impl<M: ExecMemory> Drop for PreparedExec<M> {
    fn drop(&mut self) {
        if let Some(image) = self.image.take() {
            self.memory.destroy_root(image.root_pa);
        }
        if !self.refcount.is_null() {
            self.memory.free_refcount(self.refcount);
        }
    }
}

#[cfg(test)]
mod tests;
