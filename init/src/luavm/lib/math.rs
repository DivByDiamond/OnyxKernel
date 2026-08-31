//! Lua math library (minimal MVP)

use crate::luavm::value::{Value, ValueKey};
use crate::luavm::vm::VM;
use alloc::collections::BTreeMap;

/// Register math library
pub fn register(vm: &mut VM) {
    let mut math_lib = BTreeMap::new();

    // Math constants
    math_lib.insert(ValueKey::Str("pi".into()), Value::Number(3.141592653589793));

    // Placeholders for math functions
    math_lib.insert(ValueKey::Str("abs".into()), Value::Nil);
    math_lib.insert(ValueKey::Str("floor".into()), Value::Nil);
    math_lib.insert(ValueKey::Str("ceil".into()), Value::Nil);
    math_lib.insert(ValueKey::Str("min".into()), Value::Nil);
    math_lib.insert(ValueKey::Str("max".into()), Value::Nil);
    math_lib.insert(ValueKey::Str("sqrt".into()), Value::Nil);

    vm.set_global("math".into(), Value::Table(math_lib));
}
