//! Milestone 5: Package Registry & Publishing Integration Test Suite
//! tests/pkg_publish_test.rs

use shae::pkg::{PkgError, pack_package, publish_package, unpack_package, verify_package_checksum};
use std::fs;
use std::path::PathBuf;

fn setup_test_package(name: &str, version: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("shae_pkg_test_{}", name));
    let _ = fs::remove_dir_all(&base);
    let pkg_dir = base.join("project");
    fs::create_dir_all(pkg_dir.join("src")).expect("create pkg dir");

    let manifest = format!(
        r#"[package]
name = "{}"
version = "{}"
entry = "main.shae"
description = "Test package for Shae package registry"
authors = ["Tester <test@shae.dev>"]
license = "MIT"
repository = "https://github.com/spdedsec/shae"
"#,
        name, version
    );
    fs::write(pkg_dir.join("shae.toml"), manifest).expect("write manifest");

    let main_code = r#"use { helper_fn } from "./src/helper.shae"

fn main() {
    println("Calculated: ", helper_fn(21))
}
"#;
    fs::write(pkg_dir.join("main.shae"), main_code).expect("write main");

    let helper_code = r#"fn helper_fn(x) {
    return x * 2
}
"#;
    fs::write(pkg_dir.join("src").join("helper.shae"), helper_code).expect("write helper");

    // Create ignored files that should NOT be packed
    let _ = fs::create_dir_all(pkg_dir.join("target"));
    let _ = fs::write(
        pkg_dir.join("target").join("dummy.bin"),
        "should be ignored",
    );
    let _ = fs::create_dir_all(pkg_dir.join(".git"));
    let _ = fs::write(pkg_dir.join(".git").join("config"), "git config ignored");

    (base, pkg_dir)
}

#[test]
fn test_pack_package_generates_valid_shaepkg() {
    let (base, pkg_dir) = setup_test_package("math_utils", "1.0.0");

    let pack_res = pack_package(&pkg_dir, None).expect("pack_package failed");

    assert_eq!(pack_res.package_name, "math_utils");
    assert_eq!(pack_res.version, "1.0.0");
    assert_eq!(pack_res.file_count, 3, "Expected exactly 3 files packed");
    assert!(
        pack_res.archive_path.exists(),
        "Archive file was not created"
    );
    assert_eq!(
        pack_res.sha256.len(),
        64,
        "SHA-256 must be 64 hex characters"
    );

    // Test verify checksum
    let is_valid = verify_package_checksum(&pack_res.archive_path, &pack_res.sha256)
        .expect("verify checksum failed");
    assert!(is_valid);

    // Test tampered checksum
    let fake_sha = "0000000000000000000000000000000000000000000000000000000000000000";
    let tamper_res = verify_package_checksum(&pack_res.archive_path, fake_sha);
    assert!(matches!(tamper_res, Err(PkgError::ChecksumMismatch(..))));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_unpack_package_restores_identical_tree() {
    let (base, pkg_dir) = setup_test_package("unpacker_test", "2.1.0");

    let pack_res = pack_package(&pkg_dir, None).expect("pack failed");
    let unpack_dest = base.join("extracted");

    let res = unpack_package(&pack_res.archive_path, &unpack_dest);
    assert!(res.is_ok(), "unpack failed: {:?}", res);

    let root_entry = unpack_dest.join("unpacker_test-2.1.0");
    assert!(root_entry.join("shae.toml").exists());
    assert!(root_entry.join("main.shae").exists());
    assert!(root_entry.join("src").join("helper.shae").exists());

    // Verify ignored files are absent
    assert!(!root_entry.join("target").exists());
    assert!(!root_entry.join(".git").exists());

    // Verify file content matches
    let orig_main = fs::read_to_string(pkg_dir.join("main.shae")).unwrap();
    let unpacked_main = fs::read_to_string(root_entry.join("main.shae")).unwrap();
    assert_eq!(orig_main, unpacked_main);

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_publish_package_dry_run() {
    let (base, pkg_dir) = setup_test_package("dry_run_pkg", "0.9.0");

    let pub_res = publish_package(&pkg_dir, None, None, true).expect("publish dry-run failed");
    assert!(pub_res.success);
    assert_eq!(pub_res.package_name, "dry_run_pkg");
    assert_eq!(pub_res.version, "0.9.0");
    assert_eq!(pub_res.registry_url, "dry-run");
    assert!(pub_res.message.contains("Dry run: verified and packed"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_publish_package_to_local_registry() {
    let (base, pkg_dir) = setup_test_package("local_mesh", "1.4.2");
    let registry_dir = base.join("registry");

    let reg_url = format!("file://{}", registry_dir.display());
    let pub_res = publish_package(&pkg_dir, Some(&reg_url), None, false)
        .expect("publish to local registry failed");

    assert!(pub_res.success);
    assert_eq!(pub_res.package_name, "local_mesh");
    assert_eq!(pub_res.version, "1.4.2");

    // Verify archive stored in registry
    let expected_archive = registry_dir
        .join("local_mesh")
        .join("local_mesh-1.4.2.shaepkg");
    assert!(expected_archive.exists(), "Registry archive was not copied");

    // Verify index.json metadata
    let index_file = registry_dir.join("index.json");
    assert!(index_file.exists(), "Registry index.json not found");
    let index_str = fs::read_to_string(&index_file).unwrap();
    let index_json: serde_json::Value = serde_json::from_str(&index_str).unwrap();

    let entry = &index_json["local_mesh"]["1.4.2"];
    assert_eq!(entry["version"], "1.4.2");
    assert_eq!(entry["sha256"], pub_res.sha256);
    assert_eq!(entry["files"], 3);

    let _ = fs::remove_dir_all(&base);
}
