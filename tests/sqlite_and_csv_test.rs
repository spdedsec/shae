use shae::compiler::Compiler;
use shae::parse_source;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};

fn run_shae(code: &str) -> Value {
    let program = parse_source(code).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    match vm.interpret(chunk) {
        InterpretResult::Ok(val) => val,
        InterpretResult::RuntimeError(err) => panic!("Shae runtime error: {:?}", err),
        InterpretResult::CompileError => panic!("Shae compile error"),
    }
}

#[test]
fn test_csv_parse_and_stringify_basic() {
    let script = r#"
let csv = use "std:csv"

let raw = "name,age,active,score\nAlice,30,true,95.5\nBob,25,false,82.0\nCharlie,40,true,99.25\n"
let rows = csv.parse(raw)

let first = rows[0]
let name = first["name"]
let age = first["age"]
let active = first["active"]
let score = first["score"]

let back_to_csv = csv.stringify(rows)
let re_parsed = csv.parse(back_to_csv)

[len(rows), name, age, active, score, len(re_parsed)]
"#;

    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(3));
            assert_eq!(guard[1], Value::String("Alice".into()));
            assert_eq!(guard[2], Value::Int(30));
            assert_eq!(guard[3], Value::Bool(true));
            match &guard[4] {
                Value::Float(f) | Value::Number(f) => assert!((*f - 95.5).abs() < 1e-6),
                other => panic!("Expected Float, got {:?}", other),
            }
            assert_eq!(guard[5], Value::Int(3));
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}

#[test]
fn test_csv_rfc4180_escaping_and_multiline() {
    let script = r#"
let csv = use "std:csv"

let complex_csv = "id,desc,quote\n1,\"Line 1\nLine 2\",\"He said \"\"hello\"\"\"\n2,\"Simple\",None\n"
let parsed = csv.parse(complex_csv, { typed: false })

let row1 = parsed[0]
let desc1 = row1["desc"]
let quote1 = row1["quote"]

let stringified = csv.stringify(parsed)
let re_parsed = csv.parse(stringified, { typed: false })
let re_quote1 = re_parsed[0]["quote"]

[desc1, quote1, re_quote1]
"#;

    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::String("Line 1\nLine 2".into()));
            assert_eq!(guard[1], Value::String("He said \"hello\"".into()));
            assert_eq!(guard[2], Value::String("He said \"hello\"".into()));
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}

#[test]
fn test_csv_headerless_arrays() {
    let script = r#"
let csv = use "std:csv"

let csv_text = "10;20;30\n40;50;60\n"
let table = csv.parse(csv_text, { headers: false, delimiter: ";" })
let s = csv.stringify(table, { delimiter: ";" })

[len(table), table[0][1], table[1][2], s]
"#;

    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(2));
            assert_eq!(guard[1], Value::Int(20));
            assert_eq!(guard[2], Value::Int(60));
            assert_eq!(guard[3], Value::String("10;20;30\n40;50;60\n".into()));
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}

#[test]
fn test_sqlite_in_memory_crud() {
    let script = r#"
let sqlite = use "std:sqlite"

let db = sqlite.open(":memory:")

sqlite.execute(db, "CREATE TABLE products (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, price REAL, stock INTEGER)")

sqlite.execute(db, "INSERT INTO products (name, price, stock) VALUES (?, ?, ?)", ["Laptop", 1299.99, 15])
let id1 = sqlite.last_insert_id(db)

sqlite.execute(db, "INSERT INTO products (name, price, stock) VALUES (?, ?, ?)", ["Mouse", 29.50, 50])
let id2 = sqlite.last_insert_id(db)

let rows = sqlite.query(db, "SELECT * FROM products ORDER BY id ASC")
let count = len(rows)

let p1 = rows[0]
let p1_name = p1["name"]
let p1_price = p1["price"]
let p1_stock = p1["stock"]

// Update
let updated = sqlite.execute(db, "UPDATE products SET stock = stock - 5 WHERE id = ?", [id1])

// Verify updated
let rows2 = sqlite.query(db, "SELECT stock FROM products WHERE id = ?", [id1])
let p1_new_stock = rows2[0]["stock"]

sqlite.close(db)

[id1, id2, count, p1_name, p1_price, p1_stock, updated, p1_new_stock]
"#;

    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(1));
            assert_eq!(guard[1], Value::Int(2));
            assert_eq!(guard[2], Value::Int(2));
            assert_eq!(guard[3], Value::String("Laptop".into()));
            match &guard[4] {
                Value::Float(f) | Value::Number(f) => assert!((*f - 1299.99).abs() < 1e-4),
                other => panic!("Expected Float price, got {:?}", other),
            }
            assert_eq!(guard[5], Value::Int(15));
            assert_eq!(guard[6], Value::Int(1));
            assert_eq!(guard[7], Value::Int(10));
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}

#[test]
fn test_sqlite_object_methods_and_csv_export() {
    let script = r#"
let sqlite = use "std:sqlite"
let csv = use "std:csv"

let db = sqlite.open(":memory:")
db.execute("CREATE TABLE metrics (service TEXT, p99 REAL, errors INTEGER)")

db.execute("INSERT INTO metrics (service, p99, errors) VALUES (?, ?, ?)", ["auth", 12.4, 0])
db.execute("INSERT INTO metrics (service, p99, errors) VALUES (?, ?, ?)", ["payments", 45.8, 3])
db.execute("INSERT INTO metrics (service, p99, errors) VALUES (?, ?, ?)", ["orders", 21.0, 1])

let records = db.query("SELECT service, p99, errors FROM metrics ORDER BY p99 DESC")
db.close()

let csv_export = csv.stringify(records)
let re_imported = csv.parse(csv_export)

[len(records), len(re_imported), re_imported[0]["service"], re_imported[0]["p99"]]
"#;

    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(3));
            assert_eq!(guard[1], Value::Int(3));
            assert_eq!(guard[2], Value::String("payments".into()));
            match &guard[3] {
                Value::Float(f) | Value::Number(f) => assert!((*f - 45.8).abs() < 1e-4),
                other => panic!("Expected Float, got {:?}", other),
            }
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}
