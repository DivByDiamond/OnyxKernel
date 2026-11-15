//! Deterministic completed-block-write fault injection; serialized by SERIAL.
use super::super::*;
use ::alloc::vec::Vec;
use onyx_core::errno::Errno;

pub(crate) static mut TEST_DISK: Option<Vec<u8>> = None;
static mut WRITES: usize = 0;
static mut FAIL_AFTER: usize = usize::MAX;

/// # Safety
/// Caller holds SERIAL across every access to test filesystem globals.
pub(super) unsafe fn reset_faults() {
    // SAFETY: SERIAL excludes other test disk users.
    unsafe {
        WRITES = 0;
        FAIL_AFTER = usize::MAX;
    }
}

/// # Safety
/// Caller holds SERIAL; the next write after `count` successes fails.
pub(super) unsafe fn fail_after(count: usize) {
    // SAFETY: SERIAL excludes other test disk users.
    unsafe {
        WRITES = 0;
        FAIL_AFTER = count;
    }
}

/// # Safety
/// Caller holds SERIAL across every access to test filesystem globals.
pub(super) unsafe fn writes() -> usize {
    // SAFETY: SERIAL excludes other test disk users.
    unsafe { WRITES }
}

/// # Safety
/// Caller holds SERIAL and passes an initialized block buffer.
pub(in crate::fs::onyxfs) unsafe fn read(
    blk: u32,
    buf: &mut [u8; ONYFS_BLOCK_SIZE],
) -> Option<KResult<()>> {
    // SAFETY: disk slices are bounded; SERIAL provides exclusive access.
    unsafe {
        let disk = (&raw const TEST_DISK).as_ref()?.as_ref()?;
        let off = blk as usize * ONYFS_BLOCK_SIZE;
        Some(match disk.get(off..off + ONYFS_BLOCK_SIZE) {
            Some(data) => {
                buf.copy_from_slice(data);
                Ok(())
            }
            None => Err(Errno::Io),
        })
    }
}

/// # Safety
/// Caller holds SERIAL and passes an initialized block buffer.
pub(in crate::fs::onyxfs) unsafe fn write(
    blk: u32,
    buf: &[u8; ONYFS_BLOCK_SIZE],
) -> Option<KResult<()>> {
    // SAFETY: disk slices are bounded; SERIAL provides exclusive access.
    unsafe {
        let disk = (&raw mut TEST_DISK).as_mut()?.as_mut()?;
        if WRITES >= FAIL_AFTER {
            return Some(Err(Errno::Io));
        }
        let off = blk as usize * ONYFS_BLOCK_SIZE;
        Some(match disk.get_mut(off..off + ONYFS_BLOCK_SIZE) {
            Some(data) => {
                data.copy_from_slice(buf);
                WRITES += 1;
                Ok(())
            }
            None => Err(Errno::Io),
        })
    }
}

/// # Safety
/// Caller holds SERIAL; the test volume must already be initialized.
pub(super) unsafe fn snapshot() -> Vec<u8> {
    // SAFETY: SERIAL excludes mutations while the test disk is cloned.
    unsafe {
        (&raw const TEST_DISK)
            .as_ref()
            .and_then(Option::as_ref)
            .unwrap()
            .clone()
    }
}

/// # Safety
/// Caller holds SERIAL and offset is inside the initialized test disk.
pub(super) unsafe fn flip(offset: usize) {
    // SAFETY: SERIAL excludes all other test disk users.
    unsafe {
        (&raw mut TEST_DISK)
            .as_mut()
            .and_then(Option::as_mut)
            .unwrap()[offset] ^= 1;
    }
}
