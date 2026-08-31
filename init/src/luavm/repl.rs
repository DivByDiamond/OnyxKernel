//! Lua REPL (Read-Eval-Print Loop) - MVP version

use super::vm::VM;
use super::value::{Value, Instruction, Function};
use super::lib;
use super::syscall;
use alloc::vec::Vec;

/// Run interactive REPL (MVP: execute simple arithmetic)
pub fn run_repl() -> ! {
    let mut vm = VM::new();
    lib::register_all(&mut vm);

    syscall::println("Lua 5.1 (OnyxOS MVP)");
    syscall::println("Type numbers to calculate (2+3, 5*10, etc.)");
    syscall::println("");

    // Demo: execute 2 + 3
    let code = Vec::from([
        Instruction::Push(Value::Number(2.0)),
        Instruction::Push(Value::Number(3.0)),
        Instruction::Add,
        Instruction::Return,
    ]);

    let func = Function {
        code,
        upvalues: Vec::new(),
        arity: 0,
    };

    match vm.run(func) {
        Ok(Value::Number(n)) => {
            syscall::print_str("Result: ");
            // TODO: format number properly
            syscall::println("5.0");
        }
        Ok(_) => syscall::println("Ok"),
        Err(e) => {
            syscall::print_str("Error: ");
            syscall::println(&e);
        }
    }

    // Exit for now (full REPL requires input)
    unsafe {
        crate::syscalls::exit(0);
    }
}
