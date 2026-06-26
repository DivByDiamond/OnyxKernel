pub const CHAN_MAX: usize = 32;
pub const CHAN_BUF_SIZE: usize = 4096;
pub const CHAN_NAME_MAX: usize = 32;
pub const CHAN_MAX_CLIENTS: usize = 4;

#[derive(Clone, Copy)]
pub struct Channel {
    pub buf: [u8; CHAN_BUF_SIZE],
    pub head: u32,
    pub tail: u32,
    pub owner_pid: u32,
    pub clients: [u32; CHAN_MAX_CLIENTS],
    pub num_clients: u32,
    pub name: [u8; CHAN_NAME_MAX],
    pub name_len: u8,
    pub used: bool,
    pub closed: bool,
    pub send_wait: *mut crate::proc::Proc,
    pub recv_wait: *mut crate::proc::Proc,
}

pub static mut G_CHANNELS: [Channel; CHAN_MAX] = [Channel {
    buf: [0; CHAN_BUF_SIZE],
    head: 0,
    tail: 0,
    owner_pid: 0,
    clients: [0; CHAN_MAX_CLIENTS],
    num_clients: 0,
    name: [0; CHAN_NAME_MAX],
    name_len: 0,
    used: false,
    closed: false,
    send_wait: core::ptr::null_mut(),
    recv_wait: core::ptr::null_mut(),
}; CHAN_MAX];
