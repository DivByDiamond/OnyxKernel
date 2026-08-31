//! Lua VM - stack machine implementation

use super::value::*;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};

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

    /// Peek at top of stack
    pub fn peek(&self) -> Value {
        self.stack.last().cloned().unwrap_or(Value::Nil)
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

            self.execute_instruction(instr)?;
        }
    }

    fn execute_instruction(&mut self, instr: Instruction) -> Result<(), String> {
        match instr {
            Instruction::Push(val) => self.push(val)?,

            Instruction::Pop => {
                self.pop();
            }

            Instruction::Add => {
                let b = self.pop();
                let a = self.pop();
                if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                    self.push(Value::Number(x + y))?;
                } else {
                    return Err("type error: + expects numbers".into());
                }
            }

            Instruction::Sub => {
                let b = self.pop();
                let a = self.pop();
                if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                    self.push(Value::Number(x - y))?;
                } else {
                    return Err("type error: - expects numbers".into());
                }
            }

            Instruction::Mul => {
                let b = self.pop();
                let a = self.pop();
                if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                    self.push(Value::Number(x * y))?;
                } else {
                    return Err("type error: * expects numbers".into());
                }
            }

            Instruction::Div => {
                let b = self.pop();
                let a = self.pop();
                if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                    if y == 0.0 {
                        return Err("division by zero".into());
                    }
                    self.push(Value::Number(x / y))?;
                } else {
                    return Err("type error: / expects numbers".into());
                }
            }

            Instruction::Neg => {
                let a = self.pop();
                if let Some(x) = a.as_number() {
                    self.push(Value::Number(-x))?;
                } else {
                    return Err("type error: unary - expects number".into());
                }
            }

            Instruction::Mod => {
                let b = self.pop();
                let a = self.pop();
                if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                    if y == 0.0 {
                        return Err("modulo by zero".into());
                    }
                    self.push(Value::Number(x % y))?;
                } else {
                    return Err("type error: % expects numbers".into());
                }
            }

            Instruction::Eq => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::Bool(a == b))?;
            }

            Instruction::Lt => {
                let b = self.pop();
                let a = self.pop();
                if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                    self.push(Value::Bool(x < y))?;
                } else {
                    return Err("type error: < expects numbers".into());
                }
            }

            Instruction::Le => {
                let b = self.pop();
                let a = self.pop();
                if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
                    self.push(Value::Bool(x <= y))?;
                } else {
                    return Err("type error: <= expects numbers".into());
                }
            }

            Instruction::And => {
                let b = self.pop();
                let a = self.pop();
                // Lua truthiness: only nil and false are falsy
                if a.to_bool() {
                    self.push(b)?;
                } else {
                    self.push(a)?;
                }
            }

            Instruction::Or => {
                let b = self.pop();
                let a = self.pop();
                if a.to_bool() {
                    self.push(a)?;
                } else {
                    self.push(b)?;
                }
            }

            Instruction::Not => {
                let a = self.pop();
                self.push(Value::Bool(!a.to_bool()))?;
            }

            Instruction::Return => {
                let ret = self.pop();
                self.call_stack.pop();
                if self.call_stack.is_empty() {
                    self.push(ret)?;
                } else {
                    self.push(ret)?;
                }
            }

            Instruction::GetGlobal(name) => {
                let val = self.get_global(&name);
                self.push(val)?;
            }

            Instruction::SetGlobal(name) => {
                let val = self.pop();
                self.set_global(name, val);
            }

            Instruction::GetLocal(idx) => {
                let frame = self.call_stack.last().ok_or("no call frame")?;
                let val = self.stack.get(frame.base + idx)
                    .cloned()
                    .unwrap_or(Value::Nil);
                self.push(val)?;
            }

            Instruction::SetLocal(idx) => {
                let val = self.pop();
                let frame = self.call_stack.last().ok_or("no call frame")?;
                let index = frame.base + idx;
                if index < self.stack.len() {
                    self.stack[index] = val;
                } else {
                    return Err("local index out of bounds".into());
                }
            }

            Instruction::Jump(offset) => {
                let frame = self.call_stack.last_mut().ok_or("no call frame")?;
                let new_pc = (frame.pc as isize + offset - 1) as usize;
                frame.pc = new_pc;
            }

            Instruction::JumpIf(offset) => {
                let cond = self.pop();
                if cond.to_bool() {
                    let frame = self.call_stack.last_mut().ok_or("no call frame")?;
                    let new_pc = (frame.pc as isize + offset - 1) as usize;
                    frame.pc = new_pc;
                }
            }

            Instruction::JumpIfNot(offset) => {
                let cond = self.pop();
                if !cond.to_bool() {
                    let frame = self.call_stack.last_mut().ok_or("no call frame")?;
                    let new_pc = (frame.pc as isize + offset - 1) as usize;
                    frame.pc = new_pc;
                }
            }

            Instruction::NewTable => {
                self.push(Value::Table(BTreeMap::new()))?;
            }

            Instruction::GetTable => {
                let key = self.pop();
                let table = self.pop();

                if let Value::Table(map) = table {
                    let value_key = match key {
                        Value::Number(n) => ValueKey::Int(n as i64),
                        Value::String(s) => ValueKey::Str(s),
                        _ => return Err("table key must be number or string".into()),
                    };
                    let val = map.get(&value_key).cloned().unwrap_or(Value::Nil);
                    self.push(val)?;
                } else {
                    return Err("attempt to index a non-table".into());
                }
            }

            Instruction::SetTable => {
                let val = self.pop();
                let key = self.pop();
                let table = self.pop();

                if let Value::Table(mut map) = table {
                    let value_key = match key {
                        Value::Number(n) => ValueKey::Int(n as i64),
                        Value::String(s) => ValueKey::Str(s),
                        _ => return Err("table key must be number or string".into()),
                    };
                    map.insert(value_key, val);
                    self.push(Value::Table(map))?;
                } else {
                    return Err("attempt to index a non-table".into());
                }
            }

            Instruction::Call(num_args) => {
                let func = self.pop();
                if let Value::Function(f) = func {
                    // Verify arity
                    if num_args != f.arity {
                        return Err("wrong number of arguments".into());
                    }

                    let frame = CallFrame {
                        pc: 0,
                        func: f,
                        base: self.stack.len() - num_args,
                    };
                    self.call_stack.push(frame);
                } else {
                    return Err("attempt to call a non-function".into());
                }
            }

            _ => return Err("unimplemented instruction".into()),
        }

        Ok(())
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
        let result = vm.run(Function {
            code,
            upvalues: vec![],
            arity: 0,
        }).unwrap();
        assert_eq!(result, Value::Number(5.0));
    }
}
