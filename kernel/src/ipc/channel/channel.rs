use super::types::{CHAN_BUF_SIZE, CHAN_MAX, CHAN_MAX_CLIENTS, CHAN_NAME_MAX, Channel, G_CHANNELS};
use onyx_core::errno::{Errno, KResult};

pub unsafe fn create(owner_pid: u32) -> KResult<u32> {
    for i in 0..CHAN_MAX {
        if !G_CHANNELS[i].used {
            G_CHANNELS[i] = Channel {
                buf: [0; CHAN_BUF_SIZE],
                head: 0,
                tail: 0,
                owner_pid,
                clients: [0; CHAN_MAX_CLIENTS],
                num_clients: 0,
                name: [0; CHAN_NAME_MAX],
                name_len: 0,
                used: true,
                closed: false,
                send_wait: core::ptr::null_mut(),
                recv_wait: core::ptr::null_mut(),
            };
            return Ok(i as u32);
        }
    }
    Err(Errno::NoMem)
}

pub unsafe fn create_named(name: &[u8], owner_pid: u32) -> KResult<u32> {
    if name.is_empty() || name.len() > CHAN_NAME_MAX - 1 {
        return Err(Errno::Inval);
    }
    if find_by_name(name).is_some() {
        return Err(Errno::Exist);
    }
    let id = create(owner_pid)?;
    let ch = &mut G_CHANNELS[id as usize];
    let nlen = name.len().min(CHAN_NAME_MAX - 1);
    ch.name[..nlen].copy_from_slice(&name[..nlen]);
    ch.name_len = nlen as u8;
    Ok(id)
}

pub unsafe fn find_by_name(name: &[u8]) -> Option<u32> {
    for i in 0..CHAN_MAX {
        let ch = &G_CHANNELS[i];
        if ch.used && ch.name_len as usize == name.len() && &ch.name[..ch.name_len as usize] == name
        {
            return Some(i as u32);
        }
    }
    None
}

pub unsafe fn open_by_name(name: &[u8], client_pid: u32) -> KResult<u32> {
    let id = find_by_name(name).ok_or(Errno::NoEnt)?;
    let ch = &mut G_CHANNELS[id as usize];
    if ch.num_clients as usize >= CHAN_MAX_CLIENTS {
        return Err(Errno::NoMem);
    }
    for &c in ch.clients[..ch.num_clients as usize].iter() {
        if c == client_pid {
            return Ok(id);
        }
    }
    ch.clients[ch.num_clients as usize] = client_pid;
    ch.num_clients += 1;
    Ok(id)
}

pub unsafe fn disconnect(chan_id: u32, pid: u32) {
    if chan_id as usize >= CHAN_MAX {
        return;
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used {
        return;
    }
    for i in 0..ch.num_clients as usize {
        if ch.clients[i] == pid {
            ch.clients[i] = ch.clients[ch.num_clients as usize - 1];
            ch.num_clients -= 1;
            return;
        }
    }
}

pub unsafe fn connect(chan_id: u32, client_pid: u32) -> KResult<()> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used {
        return Err(Errno::NoEnt);
    }
    if ch.num_clients as usize >= CHAN_MAX_CLIENTS {
        return Err(Errno::NoMem);
    }
    ch.clients[ch.num_clients as usize] = client_pid;
    ch.num_clients += 1;
    Ok(())
}

pub unsafe fn close(chan_id: u32) -> KResult<()> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used {
        return Err(Errno::NoEnt);
    }
    ch.closed = true;
    ch.used = false;
    Ok(())
}

pub unsafe fn named_count() -> u32 {
    let mut n = 0;
    for i in 0..CHAN_MAX {
        if G_CHANNELS[i].used && G_CHANNELS[i].name_len > 0 {
            n += 1;
        }
    }
    n
}

pub unsafe fn named_by_index(idx: u32) -> Option<(&'static [u8], u32)> {
    let mut n = 0;
    for i in 0..CHAN_MAX {
        if G_CHANNELS[i].used && G_CHANNELS[i].name_len > 0 {
            if n == idx {
                let len = G_CHANNELS[i].name_len as usize;
                return Some((&G_CHANNELS[i].name[..len], i as u32));
            }
            n += 1;
        }
    }
    None
}
