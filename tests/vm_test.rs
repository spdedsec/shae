use shae::chunk::Chunk;
use shae::opcode::OpCode;
use shae::value::Value;
use shae::vm::{VM, InterpretResult};

#[test]
fn test_vm_math() {
    let mut chunk = Chunk::new();
    
    // (1.2 + 3.4) / 2
    let c1 = chunk.add_constant(Value::Number(1.2));
    chunk.write_opcode(OpCode::Constant, 1);
    chunk.write(c1 as u8, 1);
    
    let c2 = chunk.add_constant(Value::Number(3.4));
    chunk.write_opcode(OpCode::Constant, 1);
    chunk.write(c2 as u8, 1);
    
    chunk.write_opcode(OpCode::Add, 1);
    
    let c3 = chunk.add_constant(Value::Number(2.0));
    chunk.write_opcode(OpCode::Constant, 1);
    chunk.write(c3 as u8, 1);
    
    chunk.write_opcode(OpCode::Divide, 1);
    chunk.write_opcode(OpCode::Return, 1);

    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    
    assert_eq!(result, InterpretResult::Ok(Value::Number(2.3)));
}
