use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_uchar};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use indexmap::IndexMap;
use libloading::{Library, Symbol};

use crate::value::Value;

type Sqlite3 = *mut std::ffi::c_void;
type Sqlite3Stmt = *mut std::ffi::c_void;

type Sqlite3Open = unsafe extern "C" fn(*const c_char, *mut Sqlite3) -> c_int;
type Sqlite3Close = unsafe extern "C" fn(Sqlite3) -> c_int;
type Sqlite3ErrMsg = unsafe extern "C" fn(Sqlite3) -> *const c_char;
type Sqlite3PrepareV2 = unsafe extern "C" fn(
    Sqlite3,
    *const c_char,
    c_int,
    *mut Sqlite3Stmt,
    *mut *const c_char,
) -> c_int;
type Sqlite3Step = unsafe extern "C" fn(Sqlite3Stmt) -> c_int;
type Sqlite3Finalize = unsafe extern "C" fn(Sqlite3Stmt) -> c_int;
type Sqlite3Changes = unsafe extern "C" fn(Sqlite3) -> c_int;
type Sqlite3LastInsertRowId = unsafe extern "C" fn(Sqlite3) -> i64;

type Sqlite3BindInt64 = unsafe extern "C" fn(Sqlite3Stmt, c_int, i64) -> c_int;
type Sqlite3BindDouble = unsafe extern "C" fn(Sqlite3Stmt, c_int, f64) -> c_int;
type Sqlite3BindText = unsafe extern "C" fn(
    Sqlite3Stmt,
    c_int,
    *const c_char,
    c_int,
    *const std::ffi::c_void,
) -> c_int;
type Sqlite3BindNull = unsafe extern "C" fn(Sqlite3Stmt, c_int) -> c_int;

type Sqlite3ColumnCount = unsafe extern "C" fn(Sqlite3Stmt) -> c_int;
type Sqlite3ColumnName = unsafe extern "C" fn(Sqlite3Stmt, c_int) -> *const c_char;
type Sqlite3ColumnType = unsafe extern "C" fn(Sqlite3Stmt, c_int) -> c_int;
type Sqlite3ColumnInt64 = unsafe extern "C" fn(Sqlite3Stmt, c_int) -> i64;
type Sqlite3ColumnDouble = unsafe extern "C" fn(Sqlite3Stmt, c_int) -> f64;
type Sqlite3ColumnText = unsafe extern "C" fn(Sqlite3Stmt, c_int) -> *const c_uchar;
type Sqlite3ColumnBytes = unsafe extern "C" fn(Sqlite3Stmt, c_int) -> c_int;

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;

const SQLITE_INTEGER: c_int = 1;
const SQLITE_FLOAT: c_int = 2;
const SQLITE_TEXT: c_int = 3;
const SQLITE_BLOB: c_int = 4;
const SQLITE_NULL: c_int = 5;

const SQLITE_TRANSIENT: *const std::ffi::c_void = (-1isize) as *const std::ffi::c_void;

pub struct SqliteApi {
    _lib: Library,
    open: Sqlite3Open,
    close: Sqlite3Close,
    errmsg: Sqlite3ErrMsg,
    prepare_v2: Sqlite3PrepareV2,
    step: Sqlite3Step,
    finalize: Sqlite3Finalize,
    changes: Sqlite3Changes,
    last_insert_rowid: Sqlite3LastInsertRowId,
    bind_int64: Sqlite3BindInt64,
    bind_double: Sqlite3BindDouble,
    bind_text: Sqlite3BindText,
    bind_null: Sqlite3BindNull,
    column_count: Sqlite3ColumnCount,
    column_name: Sqlite3ColumnName,
    column_type: Sqlite3ColumnType,
    column_int64: Sqlite3ColumnInt64,
    column_double: Sqlite3ColumnDouble,
    column_text: Sqlite3ColumnText,
    column_bytes: Sqlite3ColumnBytes,
}

static SQLITE_API: OnceLock<Result<SqliteApi, String>> = OnceLock::new();

fn init_sqlite_api() -> Result<SqliteApi, String> {
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &["winsqlite3.dll", "sqlite3.dll"]
    } else if cfg!(target_os = "macos") {
        &["libsqlite3.dylib", "/usr/lib/libsqlite3.dylib"]
    } else {
        &[
            "libsqlite3.so.0",
            "libsqlite3.so",
            "/usr/lib/x86_64-linux-gnu/libsqlite3.so.0",
            "/lib/x86_64-linux-gnu/libsqlite3.so.0",
        ]
    };

    let mut last_err = String::new();
    for candidate in candidates {
        unsafe {
            let lib = match Library::new(*candidate) {
                Ok(l) => l,
                Err(e) => {
                    last_err = e.to_string();
                    continue;
                }
            };

            let open: Result<Symbol<Sqlite3Open>, _> = lib.get(b"sqlite3_open\0");
            let close: Result<Symbol<Sqlite3Close>, _> = lib
                .get(b"sqlite3_close_v2\0")
                .or_else(|_| lib.get(b"sqlite3_close\0"));
            let errmsg: Result<Symbol<Sqlite3ErrMsg>, _> = lib.get(b"sqlite3_errmsg\0");
            let prepare_v2: Result<Symbol<Sqlite3PrepareV2>, _> = lib.get(b"sqlite3_prepare_v2\0");
            let step: Result<Symbol<Sqlite3Step>, _> = lib.get(b"sqlite3_step\0");
            let finalize: Result<Symbol<Sqlite3Finalize>, _> = lib.get(b"sqlite3_finalize\0");
            let changes: Result<Symbol<Sqlite3Changes>, _> = lib.get(b"sqlite3_changes\0");
            let last_insert_rowid: Result<Symbol<Sqlite3LastInsertRowId>, _> =
                lib.get(b"sqlite3_last_insert_rowid\0");
            let bind_int64: Result<Symbol<Sqlite3BindInt64>, _> = lib.get(b"sqlite3_bind_int64\0");
            let bind_double: Result<Symbol<Sqlite3BindDouble>, _> =
                lib.get(b"sqlite3_bind_double\0");
            let bind_text: Result<Symbol<Sqlite3BindText>, _> = lib.get(b"sqlite3_bind_text\0");
            let bind_null: Result<Symbol<Sqlite3BindNull>, _> = lib.get(b"sqlite3_bind_null\0");
            let column_count: Result<Symbol<Sqlite3ColumnCount>, _> =
                lib.get(b"sqlite3_column_count\0");
            let column_name: Result<Symbol<Sqlite3ColumnName>, _> =
                lib.get(b"sqlite3_column_name\0");
            let column_type: Result<Symbol<Sqlite3ColumnType>, _> =
                lib.get(b"sqlite3_column_type\0");
            let column_int64: Result<Symbol<Sqlite3ColumnInt64>, _> =
                lib.get(b"sqlite3_column_int64\0");
            let column_double: Result<Symbol<Sqlite3ColumnDouble>, _> =
                lib.get(b"sqlite3_column_double\0");
            let column_text: Result<Symbol<Sqlite3ColumnText>, _> =
                lib.get(b"sqlite3_column_text\0");
            let column_bytes: Result<Symbol<Sqlite3ColumnBytes>, _> =
                lib.get(b"sqlite3_column_bytes\0");

            if let (
                Ok(open),
                Ok(close),
                Ok(errmsg),
                Ok(prepare_v2),
                Ok(step),
                Ok(finalize),
                Ok(changes),
                Ok(last_insert_rowid),
                Ok(bind_int64),
                Ok(bind_double),
                Ok(bind_text),
                Ok(bind_null),
                Ok(column_count),
                Ok(column_name),
                Ok(column_type),
                Ok(column_int64),
                Ok(column_double),
                Ok(column_text),
                Ok(column_bytes),
            ) = (
                open,
                close,
                errmsg,
                prepare_v2,
                step,
                finalize,
                changes,
                last_insert_rowid,
                bind_int64,
                bind_double,
                bind_text,
                bind_null,
                column_count,
                column_name,
                column_type,
                column_int64,
                column_double,
                column_text,
                column_bytes,
            ) {
                return Ok(SqliteApi {
                    open: *open,
                    close: *close,
                    errmsg: *errmsg,
                    prepare_v2: *prepare_v2,
                    step: *step,
                    finalize: *finalize,
                    changes: *changes,
                    last_insert_rowid: *last_insert_rowid,
                    bind_int64: *bind_int64,
                    bind_double: *bind_double,
                    bind_text: *bind_text,
                    bind_null: *bind_null,
                    column_count: *column_count,
                    column_name: *column_name,
                    column_type: *column_type,
                    column_int64: *column_int64,
                    column_double: *column_double,
                    column_text: *column_text,
                    column_bytes: *column_bytes,
                    _lib: lib,
                });
            }
        }
    }

    Err(format!(
        "Failed to load SQLite dynamic library (tried {:?}): {}",
        candidates, last_err
    ))
}

pub fn get_sqlite_api() -> Result<&'static SqliteApi, String> {
    SQLITE_API
        .get_or_init(init_sqlite_api)
        .as_ref()
        .map_err(|e| e.clone())
}

pub struct SqliteConnection {
    db: Sqlite3,
    is_closed: bool,
}

unsafe impl Send for SqliteConnection {}
unsafe impl Sync for SqliteConnection {}

impl Drop for SqliteConnection {
    fn drop(&mut self) {
        if !self.is_closed && !self.db.is_null() {
            if let Ok(api) = get_sqlite_api() {
                unsafe {
                    (api.close)(self.db);
                }
            }
            self.is_closed = true;
        }
    }
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static CONNECTIONS: OnceLock<Mutex<HashMap<u64, Arc<Mutex<SqliteConnection>>>>> = OnceLock::new();

fn get_connections() -> &'static Mutex<HashMap<u64, Arc<Mutex<SqliteConnection>>>> {
    CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn sqlite_open(path: &str) -> Result<u64, String> {
    let api = get_sqlite_api()?;
    let c_path = CString::new(path).map_err(|e| e.to_string())?;

    let mut db: Sqlite3 = std::ptr::null_mut();
    let rc = unsafe { (api.open)(c_path.as_ptr(), &mut db) };
    if rc != SQLITE_OK {
        let msg = if !db.is_null() {
            let err_ptr = unsafe { (api.errmsg)(db) };
            let err = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { (api.close)(db) };
            err
        } else {
            format!("Failed to open SQLite database with code {}", rc)
        };
        return Err(msg);
    }

    let conn = SqliteConnection {
        db,
        is_closed: false,
    };
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    get_connections()
        .lock()
        .unwrap()
        .insert(handle, Arc::new(Mutex::new(conn)));

    Ok(handle)
}

fn bind_params(api: &SqliteApi, stmt: Sqlite3Stmt, params: &[Value]) -> Result<(), String> {
    for (i, p) in params.iter().enumerate() {
        let idx = (i + 1) as c_int;
        let rc = match p {
            Value::Int(n) => unsafe { (api.bind_int64)(stmt, idx, *n) },
            Value::Float(f) | Value::Number(f) => unsafe { (api.bind_double)(stmt, idx, *f) },
            Value::Bool(b) => unsafe { (api.bind_int64)(stmt, idx, if *b { 1 } else { 0 }) },
            Value::Null => unsafe { (api.bind_null)(stmt, idx) },
            Value::String(s) => {
                let c_str = CString::new(s.as_str()).map_err(|e| e.to_string())?;
                unsafe {
                    (api.bind_text)(
                        stmt,
                        idx,
                        c_str.as_ptr(),
                        s.len() as c_int,
                        SQLITE_TRANSIENT,
                    )
                }
            }
            other => {
                let s = other.to_display();
                let c_str = CString::new(s.as_str()).map_err(|e| e.to_string())?;
                unsafe {
                    (api.bind_text)(
                        stmt,
                        idx,
                        c_str.as_ptr(),
                        s.len() as c_int,
                        SQLITE_TRANSIENT,
                    )
                }
            }
        };
        if rc != SQLITE_OK {
            return Err(format!(
                "Failed to bind parameter at index {}: code {}",
                idx, rc
            ));
        }
    }
    Ok(())
}

pub fn sqlite_execute(handle: u64, sql: &str, params: &[Value]) -> Result<i64, String> {
    let api = get_sqlite_api()?;
    let conn_arc = get_connections()
        .lock()
        .unwrap()
        .get(&handle)
        .cloned()
        .ok_or_else(|| "SQLite connection not found or closed".to_string())?;

    let conn = conn_arc.lock().unwrap();
    if conn.is_closed {
        return Err("SQLite connection is closed".to_string());
    }

    let c_sql = CString::new(sql).map_err(|e| e.to_string())?;
    let mut stmt: Sqlite3Stmt = std::ptr::null_mut();

    let rc = unsafe {
        (api.prepare_v2)(
            conn.db,
            c_sql.as_ptr(),
            sql.len() as c_int,
            &mut stmt,
            std::ptr::null_mut(),
        )
    };
    if rc != SQLITE_OK {
        let err_ptr = unsafe { (api.errmsg)(conn.db) };
        let msg = unsafe { CStr::from_ptr(err_ptr) }
            .to_string_lossy()
            .into_owned();
        return Err(format!("SQLite prepare error: {}", msg));
    }

    if let Err(e) = bind_params(api, stmt, params) {
        unsafe { (api.finalize)(stmt) };
        return Err(e);
    }

    loop {
        let step_rc = unsafe { (api.step)(stmt) };
        if step_rc == SQLITE_DONE {
            break;
        } else if step_rc == SQLITE_ROW {
            // Drain remaining rows if execute was given a SELECT
            continue;
        } else {
            let err_ptr = unsafe { (api.errmsg)(conn.db) };
            let msg = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { (api.finalize)(stmt) };
            return Err(format!("SQLite step error: {}", msg));
        }
    }

    let changes = unsafe { (api.changes)(conn.db) };
    unsafe { (api.finalize)(stmt) };

    Ok(changes as i64)
}

pub fn sqlite_query(handle: u64, sql: &str, params: &[Value]) -> Result<Value, String> {
    let api = get_sqlite_api()?;
    let conn_arc = get_connections()
        .lock()
        .unwrap()
        .get(&handle)
        .cloned()
        .ok_or_else(|| "SQLite connection not found or closed".to_string())?;

    let conn = conn_arc.lock().unwrap();
    if conn.is_closed {
        return Err("SQLite connection is closed".to_string());
    }

    let c_sql = CString::new(sql).map_err(|e| e.to_string())?;
    let mut stmt: Sqlite3Stmt = std::ptr::null_mut();

    let rc = unsafe {
        (api.prepare_v2)(
            conn.db,
            c_sql.as_ptr(),
            sql.len() as c_int,
            &mut stmt,
            std::ptr::null_mut(),
        )
    };
    if rc != SQLITE_OK {
        let err_ptr = unsafe { (api.errmsg)(conn.db) };
        let msg = unsafe { CStr::from_ptr(err_ptr) }
            .to_string_lossy()
            .into_owned();
        return Err(format!("SQLite prepare error: {}", msg));
    }

    if let Err(e) = bind_params(api, stmt, params) {
        unsafe { (api.finalize)(stmt) };
        return Err(e);
    }

    let col_count = unsafe { (api.column_count)(stmt) };
    let mut col_names = Vec::with_capacity(col_count as usize);
    for col in 0..col_count {
        let name_ptr = unsafe { (api.column_name)(stmt, col) };
        let name = if name_ptr.is_null() {
            format!("col_{}", col)
        } else {
            unsafe { CStr::from_ptr(name_ptr) }
                .to_string_lossy()
                .into_owned()
        };
        col_names.push(name);
    }

    let mut rows: Vec<Value> = Vec::new();
    loop {
        let step_rc = unsafe { (api.step)(stmt) };
        if step_rc == SQLITE_ROW {
            let mut row_map = IndexMap::new();
            for col in 0..col_count {
                let col_type = unsafe { (api.column_type)(stmt, col) };
                let val = match col_type {
                    SQLITE_INTEGER => {
                        let v = unsafe { (api.column_int64)(stmt, col) };
                        Value::Int(v)
                    }
                    SQLITE_FLOAT => {
                        let v = unsafe { (api.column_double)(stmt, col) };
                        Value::Float(v)
                    }
                    SQLITE_TEXT => {
                        let text_ptr = unsafe { (api.column_text)(stmt, col) };
                        if text_ptr.is_null() {
                            Value::String(String::new())
                        } else {
                            let text = unsafe { CStr::from_ptr(text_ptr as *const c_char) }
                                .to_string_lossy()
                                .into_owned();
                            Value::String(text)
                        }
                    }
                    SQLITE_BLOB => {
                        let bytes_len = unsafe { (api.column_bytes)(stmt, col) } as usize;
                        let text_ptr = unsafe { (api.column_text)(stmt, col) };
                        if text_ptr.is_null() || bytes_len == 0 {
                            Value::String(String::new())
                        } else {
                            let slice = unsafe { std::slice::from_raw_parts(text_ptr, bytes_len) };
                            Value::String(String::from_utf8_lossy(slice).into_owned())
                        }
                    }
                    SQLITE_NULL => Value::Null,
                    _ => Value::Null,
                };
                row_map.insert(col_names[col as usize].clone(), val);
            }
            rows.push(Value::Map(Arc::new(RwLock::new(row_map))));
        } else if step_rc == SQLITE_DONE {
            break;
        } else {
            let err_ptr = unsafe { (api.errmsg)(conn.db) };
            let msg = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { (api.finalize)(stmt) };
            return Err(format!("SQLite step error: {}", msg));
        }
    }

    unsafe { (api.finalize)(stmt) };
    Ok(Value::Array(Arc::new(RwLock::new(rows))))
}

pub fn sqlite_last_insert_id(handle: u64) -> Result<i64, String> {
    let api = get_sqlite_api()?;
    let conn_arc = get_connections()
        .lock()
        .unwrap()
        .get(&handle)
        .cloned()
        .ok_or_else(|| "SQLite connection not found or closed".to_string())?;

    let conn = conn_arc.lock().unwrap();
    if conn.is_closed {
        return Err("SQLite connection is closed".to_string());
    }

    let id = unsafe { (api.last_insert_rowid)(conn.db) };
    Ok(id)
}

pub fn sqlite_changes(handle: u64) -> Result<i64, String> {
    let api = get_sqlite_api()?;
    let conn_arc = get_connections()
        .lock()
        .unwrap()
        .get(&handle)
        .cloned()
        .ok_or_else(|| "SQLite connection not found or closed".to_string())?;

    let conn = conn_arc.lock().unwrap();
    if conn.is_closed {
        return Err("SQLite connection is closed".to_string());
    }

    let count = unsafe { (api.changes)(conn.db) };
    Ok(count as i64)
}

pub fn sqlite_close(handle: u64) -> Result<(), String> {
    let conn_arc = get_connections()
        .lock()
        .unwrap()
        .remove(&handle)
        .ok_or_else(|| "SQLite connection not found or already closed".to_string())?;

    let mut conn = conn_arc.lock().unwrap();
    if !conn.is_closed {
        if let Ok(api) = get_sqlite_api() {
            unsafe { (api.close)(conn.db) };
        }
        conn.is_closed = true;
    }

    Ok(())
}
