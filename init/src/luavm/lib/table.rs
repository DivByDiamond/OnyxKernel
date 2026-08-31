//! Lua table library (minimal MVP)

use crate::luavm::vm::VM;
use crate::luavm::value::{Value, ValueKey};
use alloc::collections::BTreeMap;

/// Register table library
pub fn register(vm: &mut VM) {
    let mut table_lib = BTreeMap::new();

    // Placeholders for table functions
    table_lib.insert(ValueKey::Str("insert".into()), Value::Nil);
    table_lib.insert(ValueKey::Str("getn".into()), Value::Nil);
    table_lib.insert(ValueKey::Str("concat".into()), Value::Nil);

    vm.set_global("table".into(), Value::Table(table_lib));
}
