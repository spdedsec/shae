use shae::linker::NativeLinker;
use shae::run_file;
use shae::run_file_vm;
use std::fs;
use std::path::Path;
use std::process::Command;

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

#[test]
fn test_pure_shae_self_linker_dual_engine() {
    run_with_large_stack(|| {
        let path = "tests/shae/self_linker_test.shae";

        // Bytecode VM
        let vm_res = run_file_vm(path);
        assert!(
            vm_res.is_ok(),
            "self_linker_test failed on VM: {:?}",
            vm_res.err()
        );

        // AST Evaluator
        let ast_res = run_file(path);
        assert!(
            ast_res.is_ok(),
            "self_linker_test failed on AST: {:?}",
            ast_res.err()
        );
    });
}

#[test]
fn test_native_linker_elf_header_structure() {
    let header = NativeLinker::build_elf_header(0x401000, 2);
    assert_eq!(header.len(), 64);
    // Magic: \x7fELF
    assert_eq!(&header[0..4], &[127, 69, 76, 70]);
    // 64-bit, little-endian, current version
    assert_eq!(header[4], 2);
    assert_eq!(header[5], 1);
    assert_eq!(header[6], 1);
    // ET_EXEC = 2
    assert_eq!(u16::from_le_bytes([header[16], header[17]]), 2);
    // EM_X86_64 = 62
    assert_eq!(u16::from_le_bytes([header[18], header[19]]), 62);
    // Entry address
    let entry_bytes: [u8; 8] = header[24..32].try_into().unwrap();
    assert_eq!(u64::from_le_bytes(entry_bytes), 0x401000);
}

#[test]
fn test_native_linker_pe_headers_structure() {
    let headers = NativeLinker::build_pe_headers(2);
    assert!(headers.len() >= 152);
    // DOS Magic: MZ
    assert_eq!(&headers[0..2], b"MZ");
    // e_lfanew at 0x3C
    let lfanew_bytes: [u8; 4] = headers[0x3C..0x40].try_into().unwrap();
    let lfanew = u32::from_le_bytes(lfanew_bytes) as usize;
    assert_eq!(lfanew, 128);
    // PE Signature at lfanew
    assert_eq!(&headers[lfanew..lfanew + 4], b"PE\0\0");
    // COFF Machine: AMD64 (0x8664)
    let machine_bytes: [u8; 2] = headers[lfanew + 4..lfanew + 6].try_into().unwrap();
    assert_eq!(u16::from_le_bytes(machine_bytes), 0x8664);
}

#[test]
#[cfg(target_os = "linux")]
fn test_native_linker_standalone_execution() {
    let test_src = "tests/test_native_app.shae";
    let test_bin = "tests/test_native_app.bin";

    fs::write(
        test_src,
        "let a = 120\nlet b = 80\nprint(\"Native Result: \" + str(a + b))\n",
    )
    .expect("Failed to write test source");

    let bytes = NativeLinker::link_to_elf(Path::new(test_src), Path::new(test_bin), true)
        .expect("NativeLinker::link_to_elf failed");
    assert!(bytes > 0);

    // Execute the standalone binary directly as an OS process!
    let output = Command::new(format!("./{}", test_bin))
        .output()
        .expect("Failed to execute native standalone binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Native Result: 200"));

    let _ = fs::remove_file(test_src);
    let _ = fs::remove_file(test_bin);
}
