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
fn test_shae_lsm_crud_lifecycle() {
    let dir = TempDir::new("shae_lsm_crud");
    let db_path = dir.path().join("lsm_crud_db");
    let db_path_str = db_path.to_str().unwrap();

    let script1 = format!(r#"
        use {{ open, put, get, delete, has, flush, close }} from "std:db:lsm";

        let path = "{}";
        let db = open(path, {{ memtable_limit: 1024 }});
        assert(db > 0, "db handle should be positive integer");

        put(db, "customer:101", "Alice Smith");
        put(db, "customer:102", "Bob Jones");
        put(db, "customer:103", "Charlie Brown");

        assert(get(db, "customer:101") == "Alice Smith", "get customer 101");
        assert(get(db, "customer:102") == "Bob Jones", "get customer 102");
        assert(has(db, "customer:103") == true, "has customer 103");
        assert(has(db, "customer:999") == false, "nonexistent customer");

        // Flush active memtable to SSTable on disk
        let flushed = flush(db);
        assert(flushed == true, "flush must succeed");

        // Overwrite customer:101 and delete customer:102
        put(db, "customer:101", "Alice Vance");
        delete(db, "customer:102");

        assert(get(db, "customer:101") == "Alice Vance", "overwritten value");
        assert(get(db, "customer:102") == null, "deleted value must be null");
        assert(has(db, "customer:102") == false, "deleted key has() must be false");

        close(db);
    "#, db_path_str);

    let res1 = run_vm(&script1);
    assert!(res1.is_ok(), "First script execution failed: {:?}", res1.err());

    // Reopen DB from disk and verify persistence across SSTables
    let script2 = format!(r#"
        use {{ open, get, has, close }} from "std:db:lsm";

        let path = "{}";
        let db = open(path);

        assert(get(db, "customer:103") == "Charlie Brown", "recovered persisted key 103");
        assert(get(db, "customer:102") == null, "deleted key remains deleted across restart");

        close(db);
    "#, db_path_str);

    let res2 = run_vm(&script2);
    assert!(res2.is_ok(), "Second script execution failed: {:?}", res2.err());
}

#[test]
fn test_shae_lsm_range_scan_and_compaction() {
    let dir = TempDir::new("shae_lsm_scan");
    let db_path = dir.path().join("lsm_scan_db");
    let db_path_str = db_path.to_str().unwrap();

    let script = format!(r#"
        use {{ open, put, delete, scan, flush, compact, stats, close }} from "std:db:lsm";

        let path = "{}";
        let db = open(path, {{ memtable_limit: 100 }});

        // Batch 1
        put(db, "sensor:01", "temp:21.5");
        put(db, "sensor:02", "temp:22.0");
        flush(db);

        // Batch 2
        put(db, "sensor:03", "temp:23.1");
        put(db, "sensor:04", "temp:24.0");
        flush(db);

        // Batch 3
        put(db, "sensor:05", "temp:25.8");
        delete(db, "sensor:02");
        flush(db);

        // Range scan: sensor:01 to sensor:04
        let items = scan(db, "sensor:01", "sensor:04", 10);
        assert(items.len == 3, "range scan should return 3 items (sensor 02 was deleted)");
        assert(items[0][0] == "sensor:01", "first item key");
        assert(items[1][0] == "sensor:03", "second item key");
        assert(items[2][0] == "sensor:04", "third item key");

        // Inspect stats before compaction
        let s1 = stats(db);
        assert(s1.sst_count >= 3, "should have at least 3 SSTables before compaction");

        // Compact all SSTables into a single optimized SSTable
        let compacted = compact(db);
        assert(compacted >= 3, "compacted count should be at least 3");

        let s2 = stats(db);
        assert(s2.sst_count == 1, "should have exactly 1 SSTable after compaction");

        // Verify range scan after compaction
        let post_items = scan(db, "sensor:01", "sensor:05", 10);
        assert(post_items.len == 4, "all non-deleted items intact after compaction");

        close(db);
    "#, db_path_str);

    let res = run_vm(&script);
    assert!(res.is_ok(), "Range scan script execution failed: {:?}", res.err());
}
