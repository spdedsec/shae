// ==============================================================================
//  tests/release_matrix_test.rs
//  Cross-Platform Multi-Arch CI Matrix & Packaging Integrity Test Suite
// ==============================================================================

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::Archive;

// -----------------------------------------------------------------------------
// Test Helpers & RAII Scaffolding
// -----------------------------------------------------------------------------

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(suffix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "shae_rel_matrix_{}_{}_{}",
            suffix,
            std::process::id(),
            fastrand::u64(..)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("Failed to create temporary directory");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn get_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn get_build_cross_script() -> PathBuf {
    get_repo_root().join("scripts/build-cross.sh")
}

fn get_release_script() -> PathBuf {
    get_repo_root().join("scripts/release.sh")
}

fn calculate_sha256_hex(data: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, data);
    digest
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

// -----------------------------------------------------------------------------
// 1. Script Presence, Permissions, and CLI Parameter Validation
// -----------------------------------------------------------------------------

#[test]
fn test_scripts_exist_and_are_executable() {
    let build_cross = get_build_cross_script();
    let release = get_release_script();

    assert!(
        build_cross.exists(),
        "scripts/build-cross.sh must exist at {}",
        build_cross.display()
    );
    assert!(
        release.exists(),
        "scripts/release.sh must exist at {}",
        release.display()
    );

    let meta_build = fs::metadata(&build_cross).expect("metadata build-cross");
    let mode_build = meta_build.permissions().mode();
    assert!(
        (mode_build & 0o111) != 0,
        "scripts/build-cross.sh must be executable (mode: {:o})",
        mode_build
    );

    let meta_release = fs::metadata(&release).expect("metadata release");
    let mode_release = meta_release.permissions().mode();
    assert!(
        (mode_release & 0o111) != 0,
        "scripts/release.sh must be executable (mode: {:o})",
        mode_release
    );
}

#[test]
fn test_build_cross_help_and_version() {
    let script = get_build_cross_script();

    // Test --help
    let help_out = Command::new(&script)
        .arg("--help")
        .output()
        .expect("Failed to execute build-cross.sh --help");
    assert!(help_out.status.success(), "--help must exit 0");
    let help_str = String::from_utf8_lossy(&help_out.stdout);
    assert!(help_str.contains("Usage:"));
    assert!(help_str.contains("x86_64-unknown-linux-musl"));
    assert!(help_str.contains("aarch64-unknown-linux-gnu"));
    assert!(help_str.contains("aarch64-unknown-linux-musl"));
    assert!(help_str.contains("aarch64-apple-darwin"));
    assert!(help_str.contains("x86_64-apple-darwin"));
    assert!(help_str.contains("x86_64-pc-windows-msvc"));
    assert!(help_str.contains("--dry-run"));
    assert!(help_str.contains("--check"));
    assert!(help_str.contains("--release"));

    // Test --version
    let ver_out = Command::new(&script)
        .arg("--version")
        .output()
        .expect("Failed to execute build-cross.sh --version");
    assert!(ver_out.status.success(), "--version must exit 0");
    let ver_str = String::from_utf8_lossy(&ver_out.stdout);
    assert!(ver_str.contains("shae-build-cross"));
    assert!(ver_str.contains("0.8.0"));
}

#[test]
fn test_build_cross_rejects_invalid_target() {
    let script = get_build_cross_script();
    let out = Command::new(&script)
        .args(["--target", "nonexistent-target-os"])
        .output()
        .expect("Failed to execute build-cross.sh");

    assert!(
        !out.status.success(),
        "build-cross.sh must exit non-zero for invalid target"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("unsupported target"),
        "Error output must state unsupported target: {}",
        combined
    );
}

#[test]
fn test_build_cross_dry_run_matrix() {
    let script = get_build_cross_script();
    let out = Command::new(&script)
        .args(["--dry-run", "all"])
        .output()
        .expect("Failed to execute build-cross.sh --dry-run all");

    assert!(out.status.success(), "--dry-run all must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("x86_64-unknown-linux-gnu"));
    assert!(stdout.contains("x86_64-unknown-linux-musl"));
    assert!(stdout.contains("aarch64-unknown-linux-gnu"));
    assert!(stdout.contains("aarch64-unknown-linux-musl"));
    assert!(stdout.contains("aarch64-apple-darwin"));
    assert!(stdout.contains("x86_64-apple-darwin"));
    assert!(stdout.contains("x86_64-pc-windows-msvc"));
    assert!(stdout.contains("PKG_CONFIG_ALLOW_CROSS=1"));
    assert!(stdout.contains("ALSA_NO_PKG_CONFIG=1"));
    assert!(stdout.contains("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"));
}

#[test]
fn test_release_script_help_and_version() {
    let script = get_release_script();

    // Test --help
    let help_out = Command::new(&script)
        .arg("--help")
        .output()
        .expect("Failed to execute release.sh --help");
    assert!(help_out.status.success(), "--help must exit 0");
    let help_str = String::from_utf8_lossy(&help_out.stdout);
    assert!(help_str.contains("Usage:"));
    assert!(help_str.contains("--package"));
    assert!(help_str.contains("--checksums"));
    assert!(help_str.contains("--verify"));
    assert!(help_str.contains("--dry-run"));

    // Test --version
    let ver_out = Command::new(&script)
        .arg("--version")
        .output()
        .expect("Failed to execute release.sh --version");
    assert!(ver_out.status.success(), "--version must exit 0");
    let ver_str = String::from_utf8_lossy(&ver_out.stdout);
    assert!(ver_str.contains("shae-release"));
    assert!(ver_str.contains("v0.8.0"));
}

#[test]
fn test_release_script_rejects_invalid_target() {
    let script = get_release_script();
    let temp = TempDir::new("invalid_target");
    let mock_bin = temp.path.join("shae");
    fs::write(&mock_bin, b"fake binary").unwrap();

    let out = Command::new(&script)
        .args([
            "--package",
            "unsupported-unknown-platform",
            mock_bin.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute release.sh");

    assert!(
        !out.status.success(),
        "release.sh must exit non-zero for invalid target"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unsupported target"));
}

// -----------------------------------------------------------------------------
// 2. Dry-Run and Host Packaging Pipeline Execution
// -----------------------------------------------------------------------------

#[test]
fn test_release_script_dry_run_pipeline() {
    let script = get_release_script();
    let temp = TempDir::new("rel_dry_run");
    let mock_bin = temp.path.join("mock_bin");
    fs::write(&mock_bin, b"mock content").unwrap();

    let out = Command::new(&script)
        .args([
            "--dry-run",
            "--package",
            "x86_64-unknown-linux-musl",
            mock_bin.to_str().unwrap(),
            "--dist",
            temp.path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute release.sh dry-run");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[DRY RUN]"));
    assert!(stdout.contains("shae-v0.8.0-linux-x86_64-musl.tar.gz"));
}

#[test]
fn test_release_script_host_packaging_execution() {
    let script = get_release_script();
    let temp = TempDir::new("host_package");
    let mock_bin = temp.path.join("shae");
    fs::write(&mock_bin, b"#!/bin/sh\necho 'shae 0.8.0'\n").unwrap();
    let mut perm = fs::metadata(&mock_bin).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&mock_bin, perm).unwrap();

    let dist_dir = temp.path.join("dist");

    let out = Command::new(&script)
        .args([
            "--package",
            "x86_64-unknown-linux-musl",
            mock_bin.to_str().unwrap(),
            "--dist",
            dist_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute release.sh packaging");

    assert!(
        out.status.success(),
        "Packaging must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected_archive = dist_dir.join("shae-v0.8.0-linux-x86_64-musl.tar.gz");
    assert!(
        expected_archive.exists(),
        "Expected archive {} was not created",
        expected_archive.display()
    );

    let expected_sha256 = dist_dir.join("shae-v0.8.0-linux-x86_64-musl.tar.gz.sha256");
    assert!(
        expected_sha256.exists(),
        "Expected per-asset .sha256 file was not created"
    );
}

// -----------------------------------------------------------------------------
// 3. Archive Structure & Zip-Slip Security Defense
// -----------------------------------------------------------------------------

#[test]
fn test_tar_gz_archive_structure_and_zip_slip_defense() {
    let script = get_release_script();
    let temp = TempDir::new("tar_structure");
    let mock_bin = temp.path.join("shae");
    fs::write(&mock_bin, b"#!/bin/sh\necho 'shae 0.8.0'\n").unwrap();
    let mut perm = fs::metadata(&mock_bin).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&mock_bin, perm).unwrap();

    let dist_dir = temp.path.join("dist");
    let out = Command::new(&script)
        .args([
            "--package",
            "x86_64-unknown-linux-gnu",
            mock_bin.to_str().unwrap(),
            "--dist",
            dist_dir.to_str().unwrap(),
        ])
        .output()
        .expect("release.sh failed");
    assert!(out.status.success());

    let archive_path = dist_dir.join("shae-v0.8.0-linux-x86_64.tar.gz");
    assert!(archive_path.exists());

    let file = File::open(&archive_path).expect("open tar.gz");
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);

    let mut found_bin = false;
    let mut found_readme = false;
    let mut found_license = false;
    let mut found_getting_started = false;

    for entry_res in archive.entries().expect("tar entries") {
        let entry = entry_res.expect("entry");
        let path = entry.path().expect("entry path");
        let path_str = path.to_string_lossy();

        assert!(
            !path_str.contains(".."),
            "Zip-Slip vulnerability detected: path contains '..': {}",
            path_str
        );
        assert!(
            !path_str.starts_with('/'),
            "Zip-Slip vulnerability detected: path starts with '/': {}",
            path_str
        );
        assert!(
            path_str.starts_with("shae-v0.8.0-x86_64-unknown-linux-gnu/"),
            "All entries must reside in top-level directory: {}",
            path_str
        );

        if path_str.ends_with("/shae") {
            found_bin = true;
            let mode = entry.header().mode().expect("mode");
            assert!(
                (mode & 0o111) != 0,
                "Binary must have executable permissions: {:o}",
                mode
            );
        } else if path_str.ends_with("/README.md") {
            found_readme = true;
        } else if path_str.ends_with("/LICENSE") {
            found_license = true;
        } else if path_str.ends_with("/GETTING_STARTED.md") {
            found_getting_started = true;
        }
    }

    assert!(found_bin, "Archive must contain 'shae'");
    assert!(found_readme, "Archive must contain 'README.md'");
    assert!(found_license, "Archive must contain 'LICENSE'");
    assert!(
        found_getting_started,
        "Archive must contain 'GETTING_STARTED.md'"
    );
}

#[test]
fn test_windows_zip_archive_structure() {
    let script = get_release_script();
    let temp = TempDir::new("zip_structure");
    let mock_bin = temp.path.join("shae.exe");
    fs::write(&mock_bin, b"MZ_MOCK_PE32_BINARY").unwrap();

    let dist_dir = temp.path.join("dist");
    let out = Command::new(&script)
        .args([
            "--package",
            "x86_64-pc-windows-msvc",
            mock_bin.to_str().unwrap(),
            "--dist",
            dist_dir.to_str().unwrap(),
        ])
        .output()
        .expect("release.sh packaging for windows failed");
    assert!(out.status.success());

    let zip_path = dist_dir.join("shae-v0.8.0-windows-x86_64.zip");
    assert!(zip_path.exists());

    let mut zip_bytes = Vec::new();
    File::open(&zip_path)
        .unwrap()
        .read_to_end(&mut zip_bytes)
        .unwrap();
    assert_eq!(
        &zip_bytes[0..4],
        &[0x50, 0x4B, 0x03, 0x04],
        "Zip file must begin with PK magic bytes"
    );

    let extract_dir = temp.path.join("extracted_zip");
    fs::create_dir_all(&extract_dir).unwrap();

    let unzip_out = Command::new("unzip")
        .args([
            "-q",
            zip_path.to_str().unwrap(),
            "-d",
            extract_dir.to_str().unwrap(),
        ])
        .output()
        .expect("unzip command failed");
    assert!(unzip_out.status.success());

    let top_dir = extract_dir.join("shae-v0.8.0-x86_64-pc-windows-msvc");
    assert!(
        top_dir.exists(),
        "Top-level directory must exist in extracted zip"
    );
    assert!(top_dir.join("shae.exe").exists());
    assert!(top_dir.join("README.md").exists());
    assert!(top_dir.join("LICENSE").exists());
    assert!(top_dir.join("GETTING_STARTED.md").exists());
}

// -----------------------------------------------------------------------------
// 4. Checksum Manifest Format Compliance & Tamper Detection
// -----------------------------------------------------------------------------

#[test]
fn test_checksum_manifest_format_and_mathematical_parity() {
    let script = get_release_script();
    let temp = TempDir::new("checksum_parity");
    let dist_dir = temp.path.join("dist");
    fs::create_dir_all(&dist_dir).unwrap();

    let file1 = dist_dir.join("shae-v0.8.0-linux-x86_64.tar.gz");
    let file2 = dist_dir.join("shae-v0.8.0-linux-x86_64-musl.tar.gz");
    let file3 = dist_dir.join("shae-v0.8.0-windows-x86_64.zip");

    let data1 = b"RELEASE_PAYLOAD_1_LINUX_GNU";
    let data2 = b"RELEASE_PAYLOAD_2_LINUX_MUSL";
    let data3 = b"RELEASE_PAYLOAD_3_WINDOWS_MSVC";

    fs::write(&file1, data1).unwrap();
    fs::write(&file2, data2).unwrap();
    fs::write(&file3, data3).unwrap();

    let out = Command::new(&script)
        .args(["--checksums", "--dist", dist_dir.to_str().unwrap()])
        .output()
        .expect("release.sh --checksums failed");
    assert!(out.status.success());

    let upper_manifest = dist_dir.join("SHA256SUMS.txt");
    let lower_manifest = dist_dir.join("sha256sums.txt");

    assert!(upper_manifest.exists(), "SHA256SUMS.txt must exist");
    assert!(lower_manifest.exists(), "sha256sums.txt must exist");

    let upper_content = fs::read_to_string(&upper_manifest).unwrap();
    let lower_content = fs::read_to_string(&lower_manifest).unwrap();
    assert_eq!(
        upper_content, lower_content,
        "SHA256SUMS.txt and sha256sums.txt must be identical"
    );

    let expected_hash1 = calculate_sha256_hex(data1);
    let expected_hash2 = calculate_sha256_hex(data2);
    let expected_hash3 = calculate_sha256_hex(data3);

    for line in upper_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(
            parts.len(),
            2,
            "Manifest line must contain exactly hash and filename: {}",
            line
        );
        let hash = parts[0];
        let filename = parts[1];

        assert_eq!(hash.len(), 64, "SHA-256 must be 64 characters");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash must be hexadecimal"
        );
        assert_eq!(hash, hash.to_lowercase(), "Hash must be lowercase");
        assert!(
            !filename.starts_with("./"),
            "Filename must not have leading ./: {}",
            filename
        );

        match filename {
            "shae-v0.8.0-linux-x86_64.tar.gz" => assert_eq!(hash, expected_hash1),
            "shae-v0.8.0-linux-x86_64-musl.tar.gz" => assert_eq!(hash, expected_hash2),
            "shae-v0.8.0-windows-x86_64.zip" => assert_eq!(hash, expected_hash3),
            other => panic!("Unexpected filename in manifest: {}", other),
        }
    }
}

#[test]
fn test_checksum_verification_and_tamper_detection() {
    let script = get_release_script();
    let temp = TempDir::new("tamper_detect");
    let dist_dir = temp.path.join("dist");
    fs::create_dir_all(&dist_dir).unwrap();

    let mock_bin = temp.path.join("shae");
    fs::write(&mock_bin, b"#!/bin/sh\necho 'shae 0.8.0'\n").unwrap();
    let mut perm = fs::metadata(&mock_bin).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&mock_bin, perm).unwrap();

    Command::new(&script)
        .args([
            "--package",
            "x86_64-unknown-linux-musl",
            mock_bin.to_str().unwrap(),
            "--dist",
            dist_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let win_mock = temp.path.join("shae.exe");
    fs::write(&win_mock, b"MZ_MOCK_WINDOWS").unwrap();
    Command::new(&script)
        .args([
            "--package",
            "x86_64-pc-windows-msvc",
            win_mock.to_str().unwrap(),
            "--dist",
            dist_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    Command::new(&script)
        .args(["--checksums", "--dist", dist_dir.to_str().unwrap()])
        .output()
        .unwrap();

    // 1. Initial verification must pass
    let verify_clean = Command::new(&script)
        .args(["--verify", dist_dir.to_str().unwrap()])
        .output()
        .expect("verify clean");
    assert!(
        verify_clean.status.success(),
        "Verification must pass on clean assets: {}",
        String::from_utf8_lossy(&verify_clean.stderr)
    );

    // 2. Tamper Test A: Mutate one byte inside the archive
    let musl_archive = dist_dir.join("shae-v0.8.0-linux-x86_64-musl.tar.gz");
    let mut bytes = fs::read(&musl_archive).unwrap();
    bytes[10] ^= 0x5A;
    fs::write(&musl_archive, &bytes).unwrap();

    let verify_tampered = Command::new(&script)
        .args(["--verify", dist_dir.to_str().unwrap()])
        .output()
        .expect("verify tampered");
    assert!(
        !verify_tampered.status.success(),
        "Verification must fail when an archive byte is mutated!"
    );
    let tampered_err = String::from_utf8_lossy(&verify_tampered.stderr);
    assert!(
        tampered_err.contains("checksum mismatch"),
        "Error output must state checksum mismatch: {}",
        tampered_err
    );

    // 3. Tamper Test B: Append unauthorized byte
    bytes.push(0xFF);
    fs::write(&musl_archive, &bytes).unwrap();
    let verify_appended = Command::new(&script)
        .args(["--verify", dist_dir.to_str().unwrap()])
        .output()
        .expect("verify appended");
    assert!(
        !verify_appended.status.success(),
        "Verification must fail when bytes are appended!"
    );

    // 4. Tamper Test C: Corrupted manifest hash
    let manifest_path = dist_dir.join("SHA256SUMS.txt");
    let corrupt_manifest = format!(
        "0000000000000000000000000000000000000000000000000000000000000000  shae-v0.8.0-windows-x86_64.zip\n"
    );
    fs::write(&manifest_path, corrupt_manifest).unwrap();
    let verify_corrupt = Command::new(&script)
        .args(["--verify", dist_dir.to_str().unwrap()])
        .output()
        .expect("verify corrupt manifest");
    assert!(
        !verify_corrupt.status.success(),
        "Verification must fail with corrupted manifest!"
    );
}

// -----------------------------------------------------------------------------
// 5. GitHub Actions Release Workflow Schema & Syntax Integrity
// -----------------------------------------------------------------------------

#[test]
fn test_github_actions_workflow_schema_and_integrity() {
    let repo_root = get_repo_root();
    let workflow_path = repo_root.join(".github/workflows/release.yml");
    assert!(
        workflow_path.exists(),
        ".github/workflows/release.yml must exist"
    );

    let py_output = Command::new("python3")
        .args([
            "-c",
            "import yaml, json, sys; json.dump(yaml.safe_load(open(sys.argv[1])), sys.stdout)",
            workflow_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run PyYAML on release.yml");

    assert!(
        py_output.status.success(),
        "release.yml must be valid YAML syntax: {}",
        String::from_utf8_lossy(&py_output.stderr)
    );

    let val: serde_json::Value =
        serde_json::from_slice(&py_output.stdout).expect("Valid JSON AST from PyYAML");

    // 1. Triggers
    let on_block = &val["on"];
    let push_tags = on_block["push"]["tags"]
        .as_array()
        .expect("push.tags must be an array");
    let has_v_tag = push_tags.iter().any(|t| t.as_str() == Some("v*"));
    assert!(has_v_tag, "Workflow must trigger on 'v*' tags");
    assert!(
        on_block["workflow_dispatch"].is_object() || on_block["workflow_dispatch"].is_null(),
        "Workflow must include workflow_dispatch trigger"
    );

    // 2. Matrix Strategy: fail-fast false
    let fail_fast = val["jobs"]["build-binaries"]["strategy"]["fail-fast"]
        .as_bool()
        .unwrap_or(true);
    assert!(!fail_fast, "matrix strategy must set fail-fast: false");

    // 3. Matrix Targets Coverage
    let matrix_include = val["jobs"]["build-binaries"]["strategy"]["matrix"]["include"]
        .as_array()
        .expect("matrix.include must be an array");

    let targets: Vec<&str> = matrix_include
        .iter()
        .filter_map(|e| e["target"].as_str())
        .collect();

    assert!(
        targets.contains(&"x86_64-unknown-linux-gnu"),
        "Missing x86_64-unknown-linux-gnu"
    );
    assert!(
        targets.contains(&"x86_64-unknown-linux-musl"),
        "Missing x86_64-unknown-linux-musl"
    );
    assert!(
        targets.contains(&"aarch64-unknown-linux-gnu"),
        "Missing aarch64-unknown-linux-gnu"
    );
    assert!(
        targets.contains(&"aarch64-unknown-linux-musl"),
        "Missing aarch64-unknown-linux-musl"
    );
    assert!(
        targets.contains(&"aarch64-apple-darwin"),
        "Missing aarch64-apple-darwin"
    );
    assert!(
        targets.contains(&"x86_64-apple-darwin"),
        "Missing x86_64-apple-darwin"
    );
    assert!(
        targets.contains(&"x86_64-pc-windows-msvc"),
        "Missing x86_64-pc-windows-msvc"
    );

    // 4. Check macOS Universal Job
    let universal_job = &val["jobs"]["build-macos-universal"];
    assert!(
        universal_job.is_object(),
        "Must define build-macos-universal job"
    );
    let universal_steps = universal_job["steps"].as_array().expect("steps");
    let has_lipo = universal_steps
        .iter()
        .any(|s| s["run"].as_str().unwrap_or("").contains("lipo -create"));
    assert!(has_lipo, "build-macos-universal must run lipo -create");

    // 5. Check Publish Release Job
    let publish_job = &val["jobs"]["publish-release"];
    assert!(publish_job.is_object(), "Must define publish-release job");
}

// -----------------------------------------------------------------------------
// 6. Static Musl Binary Properties (ELF Header PT_INTERP Inspection)
// -----------------------------------------------------------------------------

#[test]
fn test_static_musl_binary_elf_inspection() {
    let repo_root = get_repo_root();
    let candidates = [
        repo_root.join("target/x86_64-unknown-linux-musl/release/shae"),
        repo_root.join("target/x86_64-unknown-linux-musl/debug/shae"),
        PathBuf::from("/tmp/cargo-target/x86_64-unknown-linux-musl/release/shae"),
        PathBuf::from("/tmp/cargo-target/x86_64-unknown-linux-musl/debug/shae"),
    ];

    let target_bin = candidates.iter().find(|p| p.exists());
    let bin_path = match target_bin {
        Some(p) => p,
        None => {
            eprintln!("Notice: No compiled x86_64-unknown-linux-musl binary found. Skipping ELF inspection.");
            return;
        }
    };

    let mut bytes = Vec::new();
    File::open(bin_path)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();

    assert!(bytes.len() > 64, "Binary too small for ELF header");
    assert_eq!(&bytes[0..4], b"\x7fELF", "Binary must be in ELF format");

    let e_phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes(bytes[54..56].try_into().unwrap()) as usize;
    let e_phnum = u16::from_le_bytes(bytes[56..58].try_into().unwrap()) as usize;

    let mut has_pt_interp = false;
    for i in 0..e_phnum {
        let offset = e_phoff + i * e_phentsize;
        if offset + 4 <= bytes.len() {
            let p_type = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            if p_type == 3 {
                has_pt_interp = true;
            }
        }
    }

    assert!(
        !has_pt_interp,
        "Static musl binary must NOT have a PT_INTERP program header segment (dynamic interpreter path)!"
    );
}

// -----------------------------------------------------------------------------
// 7. Installer Asset Naming Parity
// -----------------------------------------------------------------------------

#[test]
fn test_installer_asset_naming_parity() {
    let repo_root = get_repo_root();
    let install_sh = repo_root.join("install.sh");
    assert!(install_sh.exists(), "install.sh must exist");

    let content = fs::read_to_string(&install_sh).expect("read install.sh");

    let expected_patterns = [
        "shae-${TAG}-linux-x86_64.tar.gz",
        "shae-${TAG}-linux-x86_64-musl.tar.gz",
        "shae-${TAG}-linux-aarch64.tar.gz",
        "shae-${TAG}-macos-arm64.tar.gz",
        "shae-${TAG}-macos-x86_64.tar.gz",
        "shae-${TAG}-windows-x86_64.zip",
    ];

    for pat in expected_patterns {
        assert!(
            content.contains(pat),
            "install.sh must reference expected release asset pattern: {}",
            pat
        );
    }
}
