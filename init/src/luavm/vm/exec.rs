//! Instruction interpreter (stack, arithmetic, comparison, logic, tables)
//! for the Lua VM. Split from mod.rs (250-line rule); control-flow and
//! call arms delegate to control.rs. Both files share the VM's private
//! fields via the same `vm` module namespace.

use super::CallFrame;
use super::{Instruction, VM, Value};
use crate::luavm::value::ValueKey;
use alloc::collections::BTreeMap;
use alloc::string::String;

/// Interpret one instruction; control-flow arms live in control.rs.
pub(super) fn execute_instruction(vm: &mut VM, instr: Instruction) -> Result<(), String> {
    match instr {
        Instruction::Push(val) => vm.push(val)?,

        Instruction::Pop => {
            vm.pop();
        }

        Instruction::Add => {
            let b = vm.pop();
            let a = vm.pop();
            if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                vm.push(Value::Number(x + y))?;
            } else {
                return Err("type error: + expects numbers".into());
            }
        }

        Instruction::Sub => {
            let b = vm.pop();
            let a = vm.pop();
            if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                vm.push(Value::Number(x - y))?;
            } else {
                return Err("type error: - expects numbers".into());
            }
        }

        Instruction::Mul => {
            let b = vm.pop();
            let a = vm.pop();
            if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                vm.push(Value::Number(x * y))?;
            } else {
                return Err("type error: * expects numbers".into());
            }
        }

        Instruction::Div => {
            let b = vm.pop();
            let a = vm.pop();
            if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                if y == 0.0 {
                    return Err("division by zero".into());
                }
                vm.push(Value::Number(x / y))?;
            } else {
                return Err("type error: / expects numbers".into());
            }
        }

        Instruction::Neg => {
            let a = vm.pop();
            if let Some(x) = a.as_number() {
                vm.push(Value::Number(-x))?;
            } else {
                return Err("type error: unary - expects number".into());
            }
        }

        Instruction::Mod => {
            let b = vm.pop();
            let a = vm.pop();
            if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                if y == 0.0 {
                    return Err("modulo by zero".into());
                }
                vm.push(Value::Number(x % y))?;
            } else {
                return Err("type error: % expects numbers".into());
            }
        }

        Instruction::Eq => {
            let b = vm.pop();
            let a = vm.pop();
            vm.push(Value::Bool(a == b))?;
        }

        Instruction::Lt => {
            let b = vm.pop();
            let a = vm.pop();
            if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                vm.push(Value::Bool(x < y))?;
            } else {
                return Err("type error: < expects numbers".into());
            }
        }

        Instruction::Le => {
            let b = vm.pop();
            let a = vm.pop();
            if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                vm.push(Value::Bool(x <= y))?;
            } else {
                return Err("type error: <= expects numbers".into());
            }
        }

        Instruction::And => {
            let b = vm.pop();
            let a = vm.pop();
            // Lua truthiness: only nil and false are falsy
            if a.to_bool() {
                vm.push(b)?;
            } else {
                vm.push(a)?;
            }
        }

        Instruction::Or => {
            let b = vm.pop();
            let a = vm.pop();
            if a.to_bool() {
                vm.push(a)?;
            } else {
                vm.push(b)?;
            }
        }

        Instruction::Not => {
            let a = vm.pop();
            vm.push(Value::Bool(!a.to_bool()))?;
        }

        Instruction::NewTable => {
            vm.push(Value::Table(BTreeMap::new()))?;
        }

        Instruction::GetTable => {
            let key = vm.pop();
            let table = vm.pop();

            if let Value::Table(map) = table {
                let value_key = match key {
                    Value::Number(n) => ValueKey::Int(n as i64),
                    Value::String(s) => ValueKey::Str(s),
                    _ => return Err("table key must be number or string".into()),
                };
                let val = map.get(&value_key).cloned().unwrap_or(Value::Nil);
                vm.push(val)?;
            } else {
                return Err("attempt to index a non-table".into());
            }
        }

        Instruction::SetTable => {
            let val = vm.pop();
            let key = vm.pop();
            let table = vm.pop();

            if let Value::Table(mut map) = table {
                let value_key = match key {
                    Value::Number(n) => ValueKey::Int(n as i64),
                    Value::String(s) => ValueKey::Str(s),
                    _ => return Err("table key must be number or string".into()),
                };
                map.insert(value_key, val);
                vm.push(Value::Table(map))?;
            } else {
                return Err("attempt to index a non-table".into());
            }
        }

        Instruction::Call(num_args) => {
            let func = vm.pop();
            if let Value::Function(f) = func {
                // Verify arity
                if num_args != f.arity {
                    return Err("wrong number of arguments".into());
                }

                let frame = CallFrame {
                    pc: 0,
                    func: f,
                    base: vm.stack.len() - num_args,
                };
                vm.call_stack.push(frame);
            } else {
                return Err("attempt to call a non-function".into());
            }
        }

        // Control flow (Return/globals/locals/branches) is delegated to
        // control.rs; Call stays here — it is a stack operation.
        instr => return super::control::execute_control(vm, instr),
    }

    Ok(())
}
