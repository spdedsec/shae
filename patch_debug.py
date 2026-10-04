with open("tests/stack_traces.rs", "r") as f:
    content = f.read()

content = content.replace('    assert!(err_str.contains("Stack Trace:"));', '    println!("ERR STR:\\n{}", err_str);\n    assert!(err_str.contains("Stack Trace:"));')

with open("tests/stack_traces.rs", "w") as f:
    f.write(content)
