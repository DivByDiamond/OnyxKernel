use super::*;
use alloc::vec;

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
