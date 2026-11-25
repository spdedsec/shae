use shae::run_vm;
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);
impl TempDir {
    fn new(prefix: &str) -> Self {
        let p = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), fastrand::u64(..)));
        let _ = std::fs::create_dir_all(&p);
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_shae_wal_script_lifecycle() {
    let dir = TempDir::new("shae_wal_shae_test");
    let wal_path = dir.path().join("wal_shae_db");
    let wal_path_str = wal_path.to_str().unwrap();

    let script = format!(r#"
        use {{ open, put, delete, checkpoint, sync, close, recover }} from "std:wal";

        let path = "{}";
        let handle = open(path, {{ max_segment_size: 512 }});
        assert(handle > 0, "wal handle should be positive integer");

        let lsn1 = put(handle, "user:1", "Alice");
        let lsn2 = put(handle, "user:2", "Bob");
        let lsn3 = delete(handle, "user:1");
        let lsn4 = checkpoint(handle, "snap_v1");

        assert(lsn1 == 1, "lsn1 must be 1");
        assert(lsn2 == 2, "lsn2 must be 2");
        assert(lsn3 == 3, "lsn3 must be 3");
        assert(lsn4 == 4, "lsn4 must be 4");

        sync(handle);
        close(handle);

        let records = recover(path);
        assert(records.len == 4, "must recover 4 records");
        assert(records[0].key == "user:1", "first record key");
        assert(records[0].type == "PUT", "first record type");
        assert(records[0].value == "Alice", "first record value");

        assert(records[1].key == "user:2", "second record key");
        assert(records[1].type == "PUT", "second record type");
        assert(records[1].value == "Bob", "second record value");

        assert(records[2].key == "user:1", "third record key");
        assert(records[2].type == "DELETE", "third record type");

        assert(records[3].type == "CHECKPOINT", "fourth record type");
    "#, wal_path_str);

    let res = run_vm(&script);
    assert!(res.is_ok(), "Script evaluation failed: {:?}", res.err());
}

#[test]
fn test_shae_wal_segment_rotation_and_purge() {
    let dir = TempDir::new("shae_wal_rot_test");
    let wal_path = dir.path().join("wal_rot_db");
    let wal_path_str = wal_path.to_str().unwrap();

    let script = format!(r#"
        use {{ open, put, sync, purge, close, recover }} from "std:wal";

        let path = "{}";
        // Tiny segment size (90 bytes) forces rotation across segments
        let handle = open(path, {{ max_segment_size: 90 }});

        let i = 1;
        while i <= 8 {{
            put(handle, "k" + str(i), "v" + str(i));
            i = i + 1;
        }}

        sync(handle);
        let pruned = purge(handle, 4);
        assert(pruned > 0, "must have pruned older segment files");

        close(handle);

        let records = recover(path);
        // Pruned older segments, so remaining records are less than total 8 but non-zero
        assert(records.len > 0, "remaining recovered records must be non-zero");
        assert(records.len < 8, "pruned records should be excluded");
    "#, wal_path_str);

    let res = run_vm(&script);
    assert!(res.is_ok(), "Script evaluation failed: {:?}", res.err());
}
