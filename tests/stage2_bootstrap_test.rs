use shae::run_file;
use shae::run_file_vm;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

static BOOTSTRAP_LOCK: Mutex<()> = Mutex::new(());

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

fn get_shae_binary() -> std::path::PathBuf {
    if let Ok(cur) = std::env::current_exe() {
        let mut path = cur.clone();
        path.pop(); // deps
        path.pop(); // debug
        path.push("debug");
        path.push("shae");
        if path.exists() {
            return path;
        }
    }
    std::path::PathBuf::from("./target/debug/shae")
}

#[test]
fn test_stage2_bootstrap_dual_engine() {
    run_with_large_stack(|| {
        let path = "tests/shae/self_stage2_bootstrap_test.shae";

        // Bytecode VM
        let vm_res = run_file_vm(path);
        assert!(
            vm_res.is_ok(),
            "self_stage2_bootstrap_test failed on VM: {:?}",
            vm_res.err()
        );

        // AST Evaluator
        let ast_res = run_file(path);
        assert!(
            ast_res.is_ok(),
            "self_stage2_bootstrap_test failed on AST: {:?}",
            ast_res.err()
        );
    });
}

#[test]
fn test_stage0_to_stage1_to_stage2_bootstrap_pipeline_and_bit_parity() {
    let _lock = BOOTSTRAP_LOCK.lock().unwrap();

    let shae_bin = get_shae_binary();
    assert!(
        shae_bin.exists(),
        "shae CLI binary must exist at {:?}",
        shae_bin
    );

    let stage0_bin = Path::new("target/shaec");
    let stage1_shaec = Path::new("target/shaec_stage2.shaec");
    let stage2_shaec = Path::new("target/shaec_stage3.shaec");

    if stage0_bin.exists() {
        let _ = fs::remove_file(stage0_bin);
    }
    if stage1_shaec.exists() {
        let _ = fs::remove_file(stage1_shaec);
    }
    if stage2_shaec.exists() {
        let _ = fs::remove_file(stage2_shaec);
    }

    // Step 0: Link Stage 0 compiler standalone binary (shae link lib/compiler/main.shae -o target/shaec)
    let status = Command::new(&shae_bin)
        .args(&[
            "link",
            "lib/compiler/main.shae",
            "-o",
            stage0_bin.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to link Stage 0 compiler");
    assert!(status.success(), "Stage 0 linking failed");
    assert!(stage0_bin.exists(), "Stage 0 binary was not created");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(stage0_bin).unwrap().permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(stage0_bin, perms);
    }

    // Step 1: Stage 0 compiler compiles lib/compiler/main.shae -> Stage 1 bytecode (target/shaec_stage2.shaec)
    let status = Command::new(stage0_bin)
        .args(&[
            "compile",
            "lib/compiler/main.shae",
            "-o",
            stage1_shaec.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute Stage 0 compiler");
    assert!(status.success(), "Stage 1 compilation failed");
    assert!(stage1_shaec.exists(), "Stage 1 bytecode was not created");

    // Step 2: Stage 1 compiler bytecode compiles lib/compiler/main.shae -> Stage 2 bytecode (target/shaec_stage3.shaec)
    let status = Command::new(&shae_bin)
        .args(&[
            stage1_shaec.to_str().unwrap(),
            "compile",
            "lib/compiler/main.shae",
            "-o",
            stage2_shaec.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute Stage 1 bytecode compiler");
    assert!(status.success(), "Stage 2 compilation failed");
    assert!(stage2_shaec.exists(), "Stage 2 bytecode was not created");

    // Step 3: Bit-for-bit parity check between Stage 1 and Stage 2 bytecode outputs
    let b1 = fs::read(stage1_shaec).expect("Failed to read Stage 1 bytecode");
    let b2 = fs::read(stage2_shaec).expect("Failed to read Stage 2 bytecode");

    assert!(b1.len() > 1000, "Stage 1 bytecode must be substantial");
    assert_eq!(
        b1.len(),
        b2.len(),
        "Stage 1 and Stage 2 bytecode lengths must match exactly ({} vs {})",
        b1.len(),
        b2.len()
    );
    assert_eq!(
        b1, b2,
        "Bit-for-bit parity verification failed: Stage 1 and Stage 2 byte sequences are not identical"
    );
}

#[test]
fn test_stage0_stage1_compilation_parity_on_user_targets() {
    let _lock = BOOTSTRAP_LOCK.lock().unwrap();

    let shae_bin = get_shae_binary();
    assert!(shae_bin.exists(), "shae CLI binary must exist");

    let stage0_bin = Path::new("target/shaec");
    if !stage0_bin.exists() {
        let status = Command::new(&shae_bin)
            .args(&[
                "link",
                "lib/compiler/main.shae",
                "-o",
                stage0_bin.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to link Stage 0 compiler");
        assert!(status.success());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(stage0_bin).unwrap().permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(stage0_bin, perms);
        }
    }

    let stage1_shaec = Path::new("target/shaec_stage2.shaec");
    if !stage1_shaec.exists() {
        let status = Command::new(stage0_bin)
            .args(&[
                "compile",
                "lib/compiler/main.shae",
                "-o",
                stage1_shaec.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to compile Stage 1 shaec");
        assert!(status.success());
    }

    let target_sample = "tests/shae/sample_target.shae";
    let s0_out = Path::new("target/sample_from_s0.shaec");
    let s1_out = Path::new("target/sample_from_s1.shaec");

    if s0_out.exists() {
        let _ = fs::remove_file(s0_out);
    }
    if s1_out.exists() {
        let _ = fs::remove_file(s1_out);
    }

    // Compile with Stage 0
    let status_s0 = Command::new(stage0_bin)
        .args(&["compile", target_sample, "-o", s0_out.to_str().unwrap()])
        .status()
        .expect("Stage 0 compilation of target sample failed");
    assert!(status_s0.success());
    assert!(s0_out.exists());

    // Compile with Stage 1
    let status_s1 = Command::new(&shae_bin)
        .args(&[
            stage1_shaec.to_str().unwrap(),
            "compile",
            target_sample,
            "-o",
            s1_out.to_str().unwrap(),
        ])
        .status()
        .expect("Stage 1 compilation of target sample failed");
    assert!(status_s1.success());
    assert!(s1_out.exists());

    // Assert bit-for-bit parity
    let b0 = fs::read(s0_out).expect("Failed to read S0 output");
    let b1 = fs::read(s1_out).expect("Failed to read S1 output");

    assert_eq!(
        b0, b1,
        "Bit-for-bit parity mismatch on user target compiled by Stage 0 vs Stage 1"
    );

    // Verify VM executes both compiled bytecodes cleanly
    let vm_res0 = run_file_vm(s0_out.to_str().unwrap());
    assert!(
        vm_res0.is_ok(),
        "Target compiled by Stage 0 failed to run: {:?}",
        vm_res0.err()
    );

    let vm_res1 = run_file_vm(s1_out.to_str().unwrap());
    assert!(
        vm_res1.is_ok(),
        "Target compiled by Stage 1 failed to run: {:?}",
        vm_res1.err()
    );

    let _ = fs::remove_file(s0_out);
    let _ = fs::remove_file(s1_out);
}
