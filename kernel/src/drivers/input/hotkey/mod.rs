//! Kernel hotkeys: chords the desktop daemon registers, intercepted in
//! `event::dispatch` before the key reaches the TTY.
//!
//! The table has one owner pid (the process holding the fb claim, which
//! the ioctl layer checks). A matching key-down is swallowed and its id
//! queued in a small FIFO the owner drains with `FB_IOCTL_HOTKEY_POLL`;
//! the queue never blocks the input drain (full = drop newest, a repeat
//! of the id already at the tail is collapsed so key auto-repeat cannot
//! flood it). The table is cleared when the owner releases the claim or
//! exits (`fb_term::graphics::release`).
//!
//! Lock: `HK_LOCK` is a leaf — taken from the input drain (under
//! `DRAIN_LOCK`) and from the owner's ioctls, never together with another
//! lock.

mod code;
#[cfg(test)]
mod tests;

pub use code::{CODE_F_BASE, Chord, MOD_ALT, MOD_CTRL, MOD_SHIFT, decode_chord, decode_code};

use super::event::KeyCode;
use onyx_core::errno::{Errno, KResult};

pub const MAX_HOTKEYS: usize = 4;
const QUEUE_CAP: usize = 8;

struct Table {
    owner: u32,
    chords: [Option<Chord>; MAX_HOTKEYS],
    queue: [u8; QUEUE_CAP],
    head: usize,
    len: usize,
}

impl Table {
    const fn new() -> Self {
        Table {
            owner: 0,
            chords: [None; MAX_HOTKEYS],
            queue: [0; QUEUE_CAP],
            head: 0,
            len: 0,
        }
    }

    fn reset(&mut self) {
        *self = Table::new();
    }

    fn set(&mut self, pid: u32, chord: Chord) -> KResult<()> {
        if pid == 0 {
            return Err(Errno::Inval);
        }
        if self.owner != 0 && self.owner != pid {
            return Err(Errno::Busy);
        }
        self.owner = pid;
        // Same id replaces; else take the first free slot.
        let slot = self
            .chords
            .iter()
            .position(|c| c.is_some_and(|c| c.id == chord.id))
            .or_else(|| self.chords.iter().position(|c| c.is_none()))
            .ok_or(Errno::NoMem)?;
        self.chords[slot] = Some(chord);
        Ok(())
    }

    fn push(&mut self, id: u8) {
        if self.len == QUEUE_CAP {
            return;
        }
        let tail = (self.head + self.len + QUEUE_CAP - 1) % QUEUE_CAP;
        if self.len > 0 && self.queue[tail] == id {
            return;
        }
        self.queue[(self.head + self.len) % QUEUE_CAP] = id;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let id = self.queue[self.head];
        self.head = (self.head + 1) % QUEUE_CAP;
        self.len -= 1;
        Some(id)
    }

    fn matches(&self, code: KeyCode, mods: u8) -> Option<u8> {
        self.chords
            .iter()
            .flatten()
            .find(|c| c.code == code && c.mods == mods)
            .map(|c| c.id)
    }
}

static HK_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();
static mut TABLE: Table = Table::new();

/// Run `f` on the table under `HK_LOCK`.
fn with_table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    HK_LOCK.lock();
    let table = &raw mut TABLE;
    // SAFETY: HK_LOCK serializes every access to TABLE; the reference
    // lives only inside `f`, which takes no other lock.
    let r = f(unsafe { &mut *table });
    HK_LOCK.unlock();
    r
}

/// Modifier wire bits from the key.rs flag byte (shift 1|2, ctrl 4|8,
/// caps 16 — ignored — and alt 32|64).
pub fn mods_from_flags(flags: u8) -> u8 {
    let mut m = 0;
    if flags & 0x03 != 0 {
        m |= MOD_SHIFT;
    }
    if flags & 0x0C != 0 {
        m |= MOD_CTRL;
    }
    if flags & 0x60 != 0 {
        m |= MOD_ALT;
    }
    m
}

/// Register (or replace) a chord for `pid`. The caller has verified that
/// `pid` holds the framebuffer claim. A second pid is `Busy`.
pub fn register(pid: u32, arg: u64) -> KResult<()> {
    let chord = decode_chord(arg).ok_or(Errno::Inval)?;
    with_table(|t| t.set(pid, chord))
}

/// Pop the next pending hotkey id for `pid` (0 = none). Only the owner
/// may poll; anyone else is `Perm`.
pub fn poll(pid: u32) -> KResult<u32> {
    with_table(|t| {
        if t.owner == 0 || t.owner != pid {
            return Err(Errno::Perm);
        }
        Ok(t.pop().map_or(0, u32::from))
    })
}

/// Drop the table if `pid` owns it (claim release / process exit).
pub fn clear(pid: u32) {
    with_table(|t| {
        if t.owner != 0 && t.owner == pid {
            t.reset();
        }
    });
}

/// Current owner pid (0 = no table). Used to hand the foreground back to
/// the desktop when the foreground app is stopped.
pub fn owner() -> u32 {
    with_table(|t| t.owner)
}

/// Dispatch hook: true when the key event belongs to a hotkey and must
/// not reach the TTY. Only the key-down of a matching chord queues an id;
/// its key-up is swallowed too so the TTY never sees half a chord.
pub fn intercept(code: KeyCode, down: bool, flags: u8) -> bool {
    let mods = mods_from_flags(flags);
    with_table(|t| match t.matches(code, mods) {
        Some(id) => {
            if down {
                t.push(id);
            }
            true
        }
        None => false,
    })
}
