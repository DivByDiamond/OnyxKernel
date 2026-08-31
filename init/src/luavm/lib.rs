//! Lua standard library modules

pub mod string;
pub mod table;
pub mod math;

use super::vm::VM;

/// Register all standard libraries
pub fn register_all(vm: &mut VM) {
    string::register(vm);
    table::register(vm);
    math::register(vm);
}
