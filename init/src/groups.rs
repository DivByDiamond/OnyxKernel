#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

use core::arch::asm;

mod auth;
mod syscalls;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    let uid = syscalls::getuid() as u32;
    let gid = syscalls::getgid() as u32;

    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0,
        gid: 0,
        home: [0; 64],
        shell: [0; 32],
    }; auth::MAX_USERS];
    let nusers = auth::read_passwd(&mut users).unwrap_or(0);

    let mut username = [0u8; 32];
    if let Some(idx) = auth::find_user_by_uid(&users, nusers, uid) {
        let ul = users[idx].name.iter().position(|&b| b == 0).unwrap_or(32);
        username[..ul].copy_from_slice(&users[idx].name[..ul]);
    }

    let mut groups = [auth::GroupEntry {
        name: [0; 32],
        gid: 0,
        members: [0; 256],
        members_len: 0,
    }; auth::MAX_GROUPS];
    let ngroups = auth::read_groups(&mut groups).unwrap_or(0);

    let mut first = true;
    for i in 0..ngroups {
        let entry = &groups[i];
        let mut show = false;
        if entry.gid == gid {
            show = true;
        } else if !username.iter().all(|&b| b == 0) {
            show = auth::user_in_group(&username, &entry.members[..entry.members_len]);
        }
        if show {
            let gl = entry.name.iter().position(|&b| b == 0).unwrap_or(32);
            if !first {
                syscalls::write(1, b" ".as_ptr(), 1);
            }
            syscalls::write(1, entry.name.as_ptr(), gl);
            first = false;
        }
    }
    syscalls::write(1, b"\n".as_ptr(), 1);
    syscalls::exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
