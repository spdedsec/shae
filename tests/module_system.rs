use shae::{run, run_file};
use serde_json::json;
use std::fs;
use std::path::Path;

#[test]
fn test_module_system_legacy_use() {
    let math_file = "tests/test_math_legacy.shae";
    fs::write(
        math_file,
        r#"
fn add(a, b) {
    return a + b
}
let PI = 3.1415
"#,
    )
    .unwrap();

    let script = format!(
        r#"
let math = use "{}"
math.add(10, 20)
"#,
        math_file
    );

    let val = run(&script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(30));

    let _ = fs::remove_file(math_file);
}

#[test]
fn test_selective_and_aliased_imports() {
    let mod_file = "tests/test_calc_mod.shae";
    fs::write(
        mod_file,
        r#"
fn add(a, b) {
    return a + b
}
fn sub(a, b) {
    return a - b
}
let base = 100
"#,
    )
    .unwrap();

    let script = format!(
        r#"
use {{ add as plus, sub, base }} from "{}"
plus(base, sub(20, 5))
"#,
        mod_file
    );

    let val = run(&script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(115));

    let _ = fs::remove_file(mod_file);
}

#[test]
fn test_wildcard_import() {
    let mod_file = "tests/test_wildcard_mod.shae";
    fs::write(
        mod_file,
        r#"
fn multiply(a, b) {
    return a * b
}
let factor = 7
"#,
    )
    .unwrap();

    let script = format!(
        r#"
use * from "{}"
multiply(factor, 6)
"#,
        mod_file
    );

    let val = run(&script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(42));

    let _ = fs::remove_file(mod_file);
}

#[test]
fn test_relative_nested_import_resolution() {
    let dir = Path::new("tests/test_nest");
    let subdir = dir.join("sub");
    let _ = fs::create_dir_all(&subdir);

    let main_file = dir.join("main.shae");
    let sub_file = subdir.join("nested.shae");
    let sibling_file = dir.join("sibling.shae");

    fs::write(&sibling_file, "fn get_val() { return 100 }\n").unwrap();
    fs::write(
        &sub_file,
        r#"
use { get_val } from "../sibling.shae"
fn compute() {
    return get_val() * 2
}
"#,
    )
    .unwrap();
    fs::write(
        &main_file,
        r#"
use { compute } from "./sub/nested.shae"
compute()
"#,
    )
    .unwrap();

    let val = run_file(&main_file).expect("Nested relative run_file failed");
    assert_eq!(val.to_json(), json!(200));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn test_module_cache_deduplication() {
    let mod_file = "tests/test_cache_mod.shae";
    fs::write(
        mod_file,
        r#"
fn get_num() {
    return 55
}
"#,
    )
    .unwrap();

    let script = format!(
        r#"
let m1 = use "{0}"
let m2 = use "{0}"
use {{ get_num }} from "{0}"
m1.get_num() + m2.get_num() + get_num()
"#,
        mod_file
    );

    let val = run(&script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(165));

    let _ = fs::remove_file(mod_file);
}

#[test]
fn test_std_fs_namespace() {
    let test_dir = "tests/test_fs_sandbox";
    let _ = fs::remove_dir_all(test_dir);

    let script = format!(
        r#"
use {{ mkdir, write, append, read, exists, isFile, isDir, readDir, remove }} from "std:fs"

mkdir("{0}")
write("{0}/hello.txt", "Hello World")
append("{0}/hello.txt", " from Shae!")
let content = read("{0}/hello.txt")

let has_file = exists("{0}/hello.txt")
let is_f = isFile("{0}/hello.txt")
let is_d = isDir("{0}")
let entries = readDir("{0}")

remove("{0}")
let after_remove = exists("{0}")

[content, has_file, is_f, is_d, entries, after_remove]
"#,
        test_dir
    );

    let val = run(&script).expect("std:fs script failed");
    assert_eq!(
        val.to_json(),
        json!([
            "Hello World from Shae!",
            true,
            true,
            true,
            ["hello.txt"],
            false
        ])
    );
}

#[test]
fn test_std_path_namespace() {
    let script = r#"
use { join, basename, dirname, ext, isAbs } from "std:path"

let p = join("src", "modules", "main.shae")
let b = basename(p)
let d = dirname(p)
let e = ext(p)
let a = isAbs(p)

[p, b, d, e, a]
"#;

    let val = run(script).expect("std:path script failed");
    assert_eq!(
        val.to_json(),
        json!([
            "src/modules/main.shae",
            "main.shae",
            "src/modules",
            "shae",
            false
        ])
    );
}

#[test]
fn test_std_sys_namespace() {
    let script = r#"
use { platform, arch, cwd, setEnv, getEnv } from "std:sys"

setEnv("SHAE_TEST_VAR", "awesome_shae")
let v = getEnv("SHAE_TEST_VAR")
let p = platform()
let a = arch()
let c = cwd()

[v, p != "", a != "", c != ""]
"#;

    let val = run(script).expect("std:sys script failed");
    assert_eq!(val.to_json(), json!(["awesome_shae", true, true, true]));
}

#[test]
fn test_std_time_namespace() {
    let script = r#"
use { now, nowMs, sleep } from "std:time"

let t1 = now()
let ms1 = nowMs()
sleep(10)
let t2 = now()
let ms2 = nowMs()

[t2 >= t1, ms2 >= ms1]
"#;

    let val = run(script).expect("std:time script failed");
    assert_eq!(val.to_json(), json!([true, true]));
}

#[test]
fn test_use_statement_formatting() {
    let src = r#"use { add as plus, sub } from "./math.shae"
use * from "./utils.shae"
"#;
    let formatted = shae::format_source(src).expect("Formatting failed");
    assert_eq!(
        formatted.trim(),
        r#"use { add as plus, sub } from "./math.shae"
use * from "./utils.shae""#
    );
}

#[test]
fn test_circular_import_safety() {
    let file_a = "tests/test_circ_a.shae";
    let file_b = "tests/test_circ_b.shae";

    fs::write(
        file_a,
        r#"
let b = use "tests/test_circ_b.shae"
fn from_a() {
    return 10
}
fn call_b() {
    return b.from_b()
}
"#,
    )
    .unwrap();

    fs::write(
        file_b,
        r#"
let a = use "tests/test_circ_a.shae"
fn from_b() {
    return 20
}
"#,
    )
    .unwrap();

    let script = format!(
        r#"
let a = use "{}"
a.from_a() + a.call_b()
"#,
        file_a
    );

    let val = run(&script).expect("Circular import execution failed");
    assert_eq!(val.to_json(), json!(30));

    let _ = fs::remove_file(file_a);
    let _ = fs::remove_file(file_b);
}
