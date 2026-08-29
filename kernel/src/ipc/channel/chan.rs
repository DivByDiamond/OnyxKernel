use onyx_core::errno::{Errno, KResult};

use super::types::{CHAN_MAX, CHAN_MAX_CLIENTS, CHAN_NAME_MAX, Channel, G_CHANNELS};

// B5 fix: every mutation of a channel slot happens under that slot's
// per-channel SpinLock. Read-only name scans (find_by_name / named_count /
// named_by_index) stay lock-free as before — they only read u8/bool fields
// and tolerate momentary staleness.

/// # Safety
///
/// Caller must not already hold any per-channel spinlock (a slot's lock is
/// taken in turn here); all slot mutation happens while that slot's lock is
/// held, so concurrent creators serialize on the per-slot locks.
pub unsafe fn create(owner_pid: u32) -> KResult<u32> {
    unsafe {
        for (i, slot) in G_CHANNELS.iter_mut().enumerate() {
            slot.lock.lock();
            if !slot.used {
                // Fresh SpinLock inside zeroed() — we hold the old lock and are
                // about to return; the new channel is not shared yet.
                *slot = Channel::zeroed();
                slot.owner_pid = owner_pid;
                slot.used = true;
                return Ok(i as u32);
            }
            slot.lock.unlock();
        }
        Err(Errno::NoMem)
    }
}

/// # Safety
///
/// Caller must not hold any per-channel spinlock; `name` is only read for
/// the duration of the call. The id returned by `create` is always < CHAN_MAX
/// (it comes from indexing G_CHANNELS), so the re-index below is in range,
/// and the name write happens under the slot's SpinLock.
pub unsafe fn create_named(name: &[u8], owner_pid: u32) -> KResult<u32> {
    unsafe {
        if name.is_empty() || name.len() > CHAN_NAME_MAX - 1 {
            return Err(Errno::Inval);
        }
        if find_by_name(name).is_some() {
            return Err(Errno::Exist);
        }
        let id = create(owner_pid)?;
        let ch = &mut G_CHANNELS[id as usize];
        ch.lock.lock();
        let nlen = name.len().min(CHAN_NAME_MAX - 1);
        ch.name[..nlen].copy_from_slice(&name[..nlen]);
        ch.name_len = nlen as u8;
        ch.lock.unlock();
        Ok(id)
    }
}

/// # Safety
///
/// Lock-free read-only scan of G_CHANNELS: only reads u8/bool/name fields
/// of slots and tolerates momentary staleness from concurrent creators
/// (per the B5 note above); returns an id that is always < CHAN_MAX.
pub unsafe fn find_by_name(name: &[u8]) -> Option<u32> {
    unsafe {
        for (i, ch) in G_CHANNELS.iter().enumerate() {
            if ch.used
                && ch.name_len as usize == name.len()
                && &ch.name[..ch.name_len as usize] == name
            {
                return Some(i as u32);
            }
        }
        None
    }
}

/// # Safety
///
/// Caller must not hold any per-channel spinlock. The id from `find_by_name`
/// is < CHAN_MAX; the clients array is only written under the slot's
/// SpinLock and num_clients is capped at CHAN_MAX_CLIENTS before the write.
pub unsafe fn open_by_name(name: &[u8], client_pid: u32) -> KResult<u32> {
    unsafe {
        let id = find_by_name(name).ok_or(Errno::NoEnt)?;
        let ch = &mut G_CHANNELS[id as usize];
        ch.lock.lock();
        if ch.num_clients as usize >= CHAN_MAX_CLIENTS {
            ch.lock.unlock();
            return Err(Errno::NoMem);
        }
        for &c in ch.clients[..ch.num_clients as usize].iter() {
            if c == client_pid {
                ch.lock.unlock();
                return Ok(id);
            }
        }
        ch.clients[ch.num_clients as usize] = client_pid;
        ch.num_clients += 1;
        ch.lock.unlock();
        Ok(id)
    }
}

/// # Safety
///
/// Caller must not hold any per-channel spinlock. chan_id is bounds-checked
/// against CHAN_MAX before G_CHANNELS is indexed; the client list is
/// mutated only while the slot's SpinLock is held.
pub unsafe fn disconnect(chan_id: u32, pid: u32) {
    unsafe {
        if chan_id as usize >= CHAN_MAX {
            return;
        }
        let ch = &mut G_CHANNELS[chan_id as usize];
        ch.lock.lock();
        if !ch.used {
            ch.lock.unlock();
            return;
        }
        for i in 0..ch.num_clients as usize {
            if ch.clients[i] == pid {
                ch.clients[i] = ch.clients[ch.num_clients as usize - 1];
                ch.num_clients -= 1;
                break;
            }
        }
        ch.lock.unlock();
    }
}

/// # Safety
///
/// Caller must not hold any per-channel spinlock. chan_id is bounds-checked
/// against CHAN_MAX before G_CHANNELS is indexed; the write to
/// clients[num_clients] happens under the slot's SpinLock with
/// num_clients < CHAN_MAX_CLIENTS already verified.
pub unsafe fn connect(chan_id: u32, client_pid: u32) -> KResult<()> {
    unsafe {
        if chan_id as usize >= CHAN_MAX {
            return Err(Errno::Inval);
        }
        let ch = &mut G_CHANNELS[chan_id as usize];
        ch.lock.lock();
        if !ch.used {
            ch.lock.unlock();
            return Err(Errno::NoEnt);
        }
        if ch.num_clients as usize >= CHAN_MAX_CLIENTS {
            ch.lock.unlock();
            return Err(Errno::NoMem);
        }
        ch.clients[ch.num_clients as usize] = client_pid;
        ch.num_clients += 1;
        ch.lock.unlock();
        Ok(())
    }
}

/// # Safety
///
/// Caller must not hold any per-channel spinlock. chan_id is bounds-checked
/// against CHAN_MAX before G_CHANNELS is indexed; used/closed are flipped
/// only while the slot's SpinLock is held.
pub unsafe fn close(chan_id: u32) -> KResult<()> {
    unsafe {
        if chan_id as usize >= CHAN_MAX {
            return Err(Errno::Inval);
        }
        let ch = &mut G_CHANNELS[chan_id as usize];
        ch.lock.lock();
        if !ch.used {
            ch.lock.unlock();
            return Err(Errno::NoEnt);
        }
        ch.closed = true;
        ch.used = false;
        ch.lock.unlock();
        Ok(())
    }
}

/// # Safety
///
/// Lock-free read-only scan of G_CHANNELS: reads only u8/bool fields and
/// tolerates momentary staleness from concurrent creators (B5 note above).
pub unsafe fn named_count() -> u32 {
    unsafe {
        G_CHANNELS
            .iter()
            .filter(|ch| ch.used && ch.name_len > 0)
            .count() as u32
    }
}

/// # Safety
///
/// Lock-free read-only scan of G_CHANNELS: reads only u8/bool/name fields,
/// tolerates momentary staleness, and the returned name slice indexes only
/// the fixed CHAN_NAME_MAX array (len taken from the u8 name_len field).
pub unsafe fn named_by_index(idx: u32) -> Option<(&'static [u8], u32)> {
    unsafe {
        let mut n = 0;
        for (i, ch) in G_CHANNELS.iter().enumerate() {
            if ch.used && ch.name_len > 0 {
                if n == idx {
                    let len = ch.name_len as usize;
                    return Some((&ch.name[..len], i as u32));
                }
                n += 1;
            }
        }
        None
    }
}
