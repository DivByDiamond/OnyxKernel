//! Control-flow and call instructions for the Lua VM: function return,
//! global/local variable access, branches and calls. Split from exec.rs
//! (250-line rule); both files share the VM's private fields via the same
//! `vm` module namespace.

use super::{Instruction, VM, Value};
use alloc::string::String;

/// Interpret a control-flow/call instruction (see exec.rs dispatch).
pub(super) fn execute_control(vm: &mut VM, instr: Instruction) -> Result<(), String> {
    match instr {
        Instruction::Return => {
            let ret = vm.pop();
            vm.call_stack.pop();
            if vm.call_stack.is_empty() {
                vm.push(ret)?;
            } else {
                vm.push(ret)?;
            }
        }

        Instruction::GetGlobal(name) => {
            let val = vm.get_global(&name);
            vm.push(val)?;
        }

        Instruction::SetGlobal(name) => {
            let val = vm.pop();
            vm.set_global(name, val);
        }

        Instruction::GetLocal(idx) => {
            let frame = vm.call_stack.last().ok_or("no call frame")?;
            let val = vm
                .stack
                .get(frame.base + idx)
                .cloned()
                .unwrap_or(Value::Nil);
            vm.push(val)?;
        }

        Instruction::SetLocal(idx) => {
            let val = vm.pop();
            let frame = vm.call_stack.last().ok_or("no call frame")?;
            let index = frame.base + idx;
            if index < vm.stack.len() {
                vm.stack[index] = val;
            } else {
                return Err("local index out of bounds".into());
            }
        }

        Instruction::Jump(offset) => {
            let frame = vm.call_stack.last_mut().ok_or("no call frame")?;
            let new_pc = (frame.pc as isize + offset - 1) as usize;
            frame.pc = new_pc;
        }

        Instruction::JumpIf(offset) => {
            let cond = vm.pop();
            if cond.to_bool() {
                let frame = vm.call_stack.last_mut().ok_or("no call frame")?;
                let new_pc = (frame.pc as isize + offset - 1) as usize;
                frame.pc = new_pc;
            }
        }

        Instruction::JumpIfNot(offset) => {
            let cond = vm.pop();
            if !cond.to_bool() {
                let frame = vm.call_stack.last_mut().ok_or("no call frame")?;
                let new_pc = (frame.pc as isize + offset - 1) as usize;
                frame.pc = new_pc;
            }
        }

        // NewTable/GetTable/SetTable (tables) are handled in exec.rs;
        // anything reaching here is a VM bug, not an input possibility.
        _ => return Err("unimplemented instruction".into()),
    }

    Ok(())
}
