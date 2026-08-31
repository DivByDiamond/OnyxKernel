//! Lua string library (minimal MVP)

use crate::luavm::vm::VM;
use crate::luavm::value::{Value, ValueKey};
use alloc::collections::BTreeMap;

/// Register string library
pub fn register(vm: &mut VM) {
    let mut string_lib = BTreeMap::new();

    // Placeholders for string functions
    // TODO: Implement with NativeFunc when available
    string_lib.insert(ValueKey::Str("len".into()), Value::Nil);
    string_lib.insert(ValueKey::Str("sub".into()), Value::Nil);
    string_lib.insert(ValueKey::Str("upper".into()), Value::Nil);
    string_lib.insert(ValueKey::Str("lower".into()), Value::Nil);

    vm.set_global("string".into(), Value::Table(string_lib));
}
