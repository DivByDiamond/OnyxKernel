//! Lua VM - stack machine implementation

use super::value::*;
mod control;
mod exec;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

const MAX_STACK_DEPTH: usize = 1000;

pub struct VM {
    stack: Vec<Value>,
    globals: BTreeMap<String, Value>,
    call_stack: Vec<CallFrame>,
}

struct CallFrame {
    pc: usize,
    func: Function,
    base: usize, // stack base for local variables
}

impl VM {
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(256),
            globals: BTreeMap::new(),
            call_stack: Vec::new(),
        }
    }

    /// Push value onto stack
    pub fn push(&mut self, val: Value) -> Result<(), String> {
        if self.stack.len() >= MAX_STACK_DEPTH {
            return Err("stack overflow".into());
        }
        self.stack.push(val);
        Ok(())
    }

    /// Pop value from stack
    pub fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Nil)
    }

    /// Set global variable
    pub fn set_global(&mut self, name: String, val: Value) {
        self.globals.insert(name, val);
    }

    /// Get global variable
    pub fn get_global(&self, name: &str) -> Value {
        self.globals.get(name).cloned().unwrap_or(Value::Nil)
    }

    /// Execute a function
    pub fn run(&mut self, func: Function) -> Result<Value, String> {
        let frame = CallFrame {
            pc: 0,
            func,
            base: self.stack.len(),
        };
        self.call_stack.push(frame);

        loop {
            let frame = match self.call_stack.last_mut() {
                Some(f) => f,
                None => return Ok(Value::Nil),
            };

            if frame.pc >= frame.func.code.len() {
                self.call_stack.pop();
                if self.call_stack.is_empty() {
                    return Ok(self.pop());
                }
                continue;
            }

            let instr = frame.func.code[frame.pc].clone();
            frame.pc += 1;

            exec::execute_instruction(self, instr)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        let mut vm = VM::new();
        let code = vec![
            Instruction::Push(Value::Number(2.0)),
            Instruction::Push(Value::Number(3.0)),
            Instruction::Add,
            Instruction::Return,
        ];
        let result = vm.run(Function { code, arity: 0 }).unwrap();
        assert_eq!(result, Value::Number(5.0));
    }
}
