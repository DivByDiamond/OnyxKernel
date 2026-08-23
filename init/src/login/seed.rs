use crate::auth;

pub unsafe fn seed_root_account() {
    let home = b"/users/root";
    let shell = b"/bin/osh";
    let _ = auth::update_passwd_entry(b"root", 0, 0, home, shell);
    // Empty password: login accepts a bare Enter for this account (single-board
    // appliance semantics, same as Minux's passwordless root).
    let _ = auth::update_shadow_password(b"root", b"");

    let mut mkdir_buf = [0u8; 64];
    mkdir_buf[..11].copy_from_slice(b"/users/root");
    let _ = crate::syscalls::mkdir(mkdir_buf.as_ptr());
}
