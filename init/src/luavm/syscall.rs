//! Syscall bindings for Lua (minimal MVP)

use super::vm::VM;
use super::value::{Value, ValueKey};
use crate::syscalls;
use alloc::collections::BTreeMap;

/// Register syscall bindings (io.write for MVP)
pub fn register(vm: &mut VM) {
    let mut io_lib = BTreeMap::new();

    // io.write placeholder
    // TODO: Implement with NativeFunc when available
    io_lib.insert(ValueKey::Str("write".into()), Value::Nil);

    vm.set_global("io".into(), Value::Table(io_lib));
}

/// Helper to write string to stdout (for REPL)
pub fn print_str(s: &str) {
    unsafe {
        syscalls::write(1, s.as_ptr(), s.len());
    }
}

/// Helper to write line to stdout
pub fn println(s: &str) {
    print_str(s);
    print_str("\n");
}
