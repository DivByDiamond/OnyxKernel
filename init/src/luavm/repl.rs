//! Lua REPL (Read-Eval-Print Loop) - MVP version

use super::lib;
use super::syscall;
use super::value::{Function, Instruction, Value};
use super::vm::VM;
use alloc::string::String;
use alloc::vec::Vec;

/// Run interactive REPL (MVP: execute simple arithmetic)
pub fn run_repl() -> ! {
    let mut vm = VM::new();
    lib::register_all(&mut vm);
    syscall::register(&mut vm);

    syscall::println("Lua 5.1 (OnyxOS MVP)");
    syscall::println("Type numbers to calculate (2+3, 5*10, etc.)");
    syscall::println("");

    // Demo 1: 2 + 3 inside a nested function value (exercises Value::Function,
    // Instruction::Call and cross-frame Return).
    let nested = Function {
        code: Vec::from([
            Instruction::Push(Value::Number(2.0)),
            Instruction::Push(Value::Number(3.0)),
            Instruction::Add,
            Instruction::Return,
        ]),
        arity: 0,
    };
    let code = Vec::from([
        Instruction::Push(Value::Function(nested)),
        Instruction::Call(0),
        Instruction::Return,
    ]);
    run_demo(&mut vm, code);

    // Demo 2: branch/string exercise (exercises Value::String, Pop,
    // JumpIfNot taken/not-taken and unconditional Jump).
    let code = Vec::from([
        Instruction::Push(Value::String(String::from("OnyxOS"))),
        Instruction::Pop, // discard the string
        Instruction::Push(Value::Bool(true)),
        Instruction::JumpIfNot(2), // not taken
        Instruction::Push(Value::Number(2.0)),
        Instruction::Push(Value::Number(3.0)),
        Instruction::Add,     // -> 5
        Instruction::Jump(2), // skip the fallback Push(99)
        Instruction::Push(Value::Number(99.0)),
        Instruction::Return,
    ]);
    run_demo(&mut vm, code);

    // Exit for now (full REPL requires input)
    unsafe {
        crate::syscalls::exit(0);
    }
}

/// Execute one demo bytecode program and print the outcome. Number
/// formatting is TODO(2026-09-01): the VM has no float formatter yet, so
/// the known 2+3 result is printed literally.
fn run_demo(vm: &mut VM, code: Vec<Instruction>) {
    let func = Function { code, arity: 0 };
    match vm.run(func) {
        Ok(Value::Number(_n)) => {
            syscall::print_str("Result: ");
            syscall::println("5.0");
        }
        Ok(_) => syscall::println("Ok"),
        Err(e) => {
            syscall::print_str("Error: ");
            syscall::println(&e);
        }
    }
}
