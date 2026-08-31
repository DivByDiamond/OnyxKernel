//! Lua value types and instruction set

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Lua value types
#[derive(Clone, Debug)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    Table(BTreeMap<ValueKey, Value>),
    Function(Function),
    // TODO: Add NativeFunc later
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            _ => false,
        }
    }
}

/// Keys for Lua tables (only int and string supported)
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueKey {
    Int(i64),
    Str(String),
}

/// Lua function (bytecode + upvalues)
#[derive(Clone, Debug)]
pub struct Function {
    pub code: Vec<Instruction>,
    pub upvalues: Vec<Value>,
    pub arity: usize, // number of parameters
}

// TODO: Native function support
// pub type NativeFn = fn(&[Value]) -> Result<Value, String>;

/// Lua VM instructions
#[derive(Clone, Debug)]
pub enum Instruction {
    // Stack operations
    Push(Value),
    Pop,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,

    // Comparison
    Eq,
    Lt,
    Le,

    // Logical
    And,
    Or,
    Not,

    // Variables
    GetGlobal(String),
    SetGlobal(String),
    GetLocal(usize),
    SetLocal(usize),

    // Tables
    NewTable,
    GetTable,
    SetTable,

    // Functions
    Call(usize), // num args
    Return,

    // Control flow
    Jump(isize),
    JumpIf(isize),
    JumpIfNot(isize),
}

impl Value {
    /// Convert to boolean (Lua truthiness: only nil and false are falsy)
    pub fn to_bool(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    /// Try to convert to number
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to convert to string
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}
