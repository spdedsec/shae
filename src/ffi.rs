use crate::ast::Span;
use crate::eval::RuntimeError;
use crate::value::Value;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiType {
    Void,
    Bool,
    I32,
    I64,
    F32,
    F64,
    String,
    Pointer,
}

impl FromStr for FfiType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "void" | "null" => Ok(FfiType::Void),
            "bool" | "boolean" => Ok(FfiType::Bool),
            "int" | "i32" | "int32" | "c_int" => Ok(FfiType::I32),
            "i64" | "int64" | "long" | "c_long" | "usize" | "isize" => Ok(FfiType::I64),
            "float" | "f32" => Ok(FfiType::F32),
            "double" | "f64" | "number" => Ok(FfiType::F64),
            "string" | "str" | "cstr" | "char*" => Ok(FfiType::String),
            "ptr" | "pointer" | "void*" => Ok(FfiType::Pointer),
            other => Err(format!("Unknown FFI type '{}'", other)),
        }
    }
}

impl FfiType {
    pub fn size(&self) -> usize {
        match self {
            FfiType::Void => 0,
            FfiType::Bool => 1,
            FfiType::I32 | FfiType::F32 => 4,
            FfiType::I64 | FfiType::F64 | FfiType::String | FfiType::Pointer => 8,
        }
    }
}

pub struct DynamicLibrary {
    pub path: String,
    pub lib: Arc<libloading::Library>,
}

unsafe impl Send for DynamicLibrary {}
unsafe impl Sync for DynamicLibrary {}

impl DynamicLibrary {
    pub fn open(path_str: &str) -> Result<Self, String> {
        let candidate = if path_str.is_empty() || path_str == "c" || path_str == "libc" {
            #[cfg(target_os = "linux")]
            {
                "libc.so.6"
            }
            #[cfg(target_os = "macos")]
            {
                "libSystem.B.dylib"
            }
            #[cfg(target_os = "windows")]
            {
                "msvcrt.dll"
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            {
                "libc.so"
            }
        } else {
            path_str
        };

        unsafe {
            libloading::Library::new(candidate)
                .map(|l| DynamicLibrary {
                    path: candidate.to_string(),
                    lib: Arc::new(l),
                })
                .map_err(|e| format!("Failed to open dynamic library '{}': {}", candidate, e))
        }
    }

    #[cfg(unix)]
    pub fn this() -> Result<Self, String> {
        use libloading::os::unix::Library as UnixLibrary;
        let unix_lib = UnixLibrary::this();
        Ok(DynamicLibrary {
            path: "<process>".to_string(),
            lib: Arc::new(unix_lib.into()),
        })
    }

    #[cfg(not(unix))]
    pub fn this() -> Result<Self, String> {
        Self::open("")
    }

    pub fn get_symbol(
        &self,
        name: &str,
        param_types: Vec<FfiType>,
        return_type: FfiType,
    ) -> Result<DynamicFunction, String> {
        let symbol_ptr = unsafe {
            let sym_bytes = name.as_bytes();
            let sym: libloading::Symbol<*const ()> = self
                .lib
                .get(sym_bytes)
                .map_err(|e| format!("Symbol '{}' not found in '{}': {}", name, self.path, e))?;
            *sym
        };

        Ok(DynamicFunction {
            lib: self.lib.clone(),
            name: name.to_string(),
            symbol_ptr,
            param_types,
            return_type,
        })
    }
}

pub struct DynamicFunction {
    pub lib: Arc<libloading::Library>,
    pub name: String,
    pub symbol_ptr: *const (),
    pub param_types: Vec<FfiType>,
    pub return_type: FfiType,
}

unsafe impl Send for DynamicFunction {}
unsafe impl Sync for DynamicFunction {}

enum RawArg {
    Int(u64),
    Float(f64),
}

impl DynamicFunction {
    pub fn invoke(&self, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
        if args.len() != self.param_types.len() {
            return Err(RuntimeError::new(format!(
                "FFI function '{}' expects {} arguments, but got {}",
                self.name,
                self.param_types.len(),
                args.len()
            ))
            .at(span));
        }

        let mut raw_args = Vec::with_capacity(args.len());
        let mut string_guards = Vec::new();

        for (i, (arg_val, expected_type)) in args.iter().zip(&self.param_types).enumerate() {
            match expected_type {
                FfiType::I32 => {
                    let n = match arg_val {
                        Value::Int(i) => *i as i32,
                        Value::Float(f) | Value::Number(f) => *f as i32,
                        other => {
                            return Err(RuntimeError::new(format!(
                                "Argument {} of '{}' expects int, got {}",
                                i,
                                self.name,
                                other.type_name()
                            ))
                            .at(span));
                        }
                    };
                    raw_args.push(RawArg::Int(n as u64));
                }
                FfiType::I64 => {
                    let n = match arg_val {
                        Value::Int(i) => *i,
                        Value::Float(f) | Value::Number(f) => *f as i64,
                        other => {
                            return Err(RuntimeError::new(format!(
                                "Argument {} of '{}' expects int64, got {}",
                                i,
                                self.name,
                                other.type_name()
                            ))
                            .at(span));
                        }
                    };
                    raw_args.push(RawArg::Int(n as u64));
                }
                FfiType::Bool => {
                    let b = arg_val.is_truthy();
                    raw_args.push(RawArg::Int(if b { 1 } else { 0 }));
                }
                FfiType::Pointer => {
                    let ptr_val = match arg_val {
                        Value::Int(i) => *i as usize as u64,
                        Value::Null => 0u64,
                        other => {
                            return Err(RuntimeError::new(format!(
                                "Argument {} of '{}' expects pointer (integer address), got {}",
                                i,
                                self.name,
                                other.type_name()
                            ))
                            .at(span));
                        }
                    };
                    raw_args.push(RawArg::Int(ptr_val));
                }
                FfiType::String => {
                    let s = match arg_val {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let cstring = CString::new(s).map_err(|e| {
                        RuntimeError::new(format!("CString conversion failed: {}", e)).at(span)
                    })?;
                    let ptr = cstring.as_ptr() as usize as u64;
                    string_guards.push(cstring);
                    raw_args.push(RawArg::Int(ptr));
                }
                FfiType::F32 => {
                    let n = match arg_val {
                        Value::Float(f) | Value::Number(f) => *f,
                        Value::Int(i) => *i as f64,
                        other => {
                            return Err(RuntimeError::new(format!(
                                "Argument {} of '{}' expects float, got {}",
                                i,
                                self.name,
                                other.type_name()
                            ))
                            .at(span));
                        }
                    };
                    raw_args.push(RawArg::Float(n));
                }
                FfiType::F64 => {
                    let n = match arg_val {
                        Value::Float(f) | Value::Number(f) => *f,
                        Value::Int(i) => *i as f64,
                        other => {
                            return Err(RuntimeError::new(format!(
                                "Argument {} of '{}' expects double, got {}",
                                i,
                                self.name,
                                other.type_name()
                            ))
                            .at(span));
                        }
                    };
                    raw_args.push(RawArg::Float(n));
                }
                FfiType::Void => {
                    raw_args.push(RawArg::Int(0));
                }
            }
        }

        let is_float_ret = matches!(self.return_type, FfiType::F32 | FfiType::F64);
        let any_float_arg = raw_args.iter().any(|a| matches!(a, RawArg::Float(_)));

        let fn_ptr = self.symbol_ptr;

        if any_float_arg || is_float_ret {
            self.invoke_float(raw_args, fn_ptr, span)
        } else {
            self.invoke_int(raw_args, fn_ptr, span)
        }
    }

    fn invoke_int(
        &self,
        raw_args: Vec<RawArg>,
        fn_ptr: *const (),
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let ints: Vec<u64> = raw_args
            .into_iter()
            .map(|a| match a {
                RawArg::Int(i) => i,
                RawArg::Float(f) => f as u64,
            })
            .collect();

        unsafe {
            match ints.len() {
                0 => match self.return_type {
                    FfiType::Void => {
                        let f: extern "C" fn() = std::mem::transmute(fn_ptr);
                        f();
                        Ok(Value::Null)
                    }
                    _ => {
                        let f: extern "C" fn() -> u64 = std::mem::transmute(fn_ptr);
                        self.convert_int_return(f())
                    }
                },
                1 => match self.return_type {
                    FfiType::Void => {
                        let f: extern "C" fn(u64) = std::mem::transmute(fn_ptr);
                        f(ints[0]);
                        Ok(Value::Null)
                    }
                    _ => {
                        let f: extern "C" fn(u64) -> u64 = std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(ints[0]))
                    }
                },
                2 => match self.return_type {
                    FfiType::Void => {
                        let f: extern "C" fn(u64, u64) = std::mem::transmute(fn_ptr);
                        f(ints[0], ints[1]);
                        Ok(Value::Null)
                    }
                    _ => {
                        let f: extern "C" fn(u64, u64) -> u64 = std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(ints[0], ints[1]))
                    }
                },
                3 => match self.return_type {
                    FfiType::Void => {
                        let f: extern "C" fn(u64, u64, u64) = std::mem::transmute(fn_ptr);
                        f(ints[0], ints[1], ints[2]);
                        Ok(Value::Null)
                    }
                    _ => {
                        let f: extern "C" fn(u64, u64, u64) -> u64 = std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(ints[0], ints[1], ints[2]))
                    }
                },
                4 => match self.return_type {
                    FfiType::Void => {
                        let f: extern "C" fn(u64, u64, u64, u64) = std::mem::transmute(fn_ptr);
                        f(ints[0], ints[1], ints[2], ints[3]);
                        Ok(Value::Null)
                    }
                    _ => {
                        let f: extern "C" fn(u64, u64, u64, u64) -> u64 =
                            std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(ints[0], ints[1], ints[2], ints[3]))
                    }
                },
                5 => match self.return_type {
                    FfiType::Void => {
                        let f: extern "C" fn(u64, u64, u64, u64, u64) = std::mem::transmute(fn_ptr);
                        f(ints[0], ints[1], ints[2], ints[3], ints[4]);
                        Ok(Value::Null)
                    }
                    _ => {
                        let f: extern "C" fn(u64, u64, u64, u64, u64) -> u64 =
                            std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(ints[0], ints[1], ints[2], ints[3], ints[4]))
                    }
                },
                6 => match self.return_type {
                    FfiType::Void => {
                        let f: extern "C" fn(u64, u64, u64, u64, u64, u64) =
                            std::mem::transmute(fn_ptr);
                        f(ints[0], ints[1], ints[2], ints[3], ints[4], ints[5]);
                        Ok(Value::Null)
                    }
                    _ => {
                        let f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64 =
                            std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(
                            ints[0], ints[1], ints[2], ints[3], ints[4], ints[5],
                        ))
                    }
                },
                n => Err(RuntimeError::new(format!(
                    "FFI functions with {} arguments are not currently supported (max 6)",
                    n
                ))
                .at(span)),
            }
        }
    }

    fn invoke_float(
        &self,
        raw_args: Vec<RawArg>,
        fn_ptr: *const (),
        span: Span,
    ) -> Result<Value, RuntimeError> {
        unsafe {
            match raw_args.len() {
                0 => {
                    let f: extern "C" fn() -> f64 = std::mem::transmute(fn_ptr);
                    Ok(Value::Float(f()))
                }
                1 => match (&raw_args[0], &self.return_type) {
                    (RawArg::Float(f_val), FfiType::F64 | FfiType::F32) => {
                        let f: extern "C" fn(f64) -> f64 = std::mem::transmute(fn_ptr);
                        Ok(Value::Float(f(*f_val)))
                    }
                    (RawArg::Float(f_val), FfiType::Void) => {
                        let f: extern "C" fn(f64) = std::mem::transmute(fn_ptr);
                        f(*f_val);
                        Ok(Value::Null)
                    }
                    (RawArg::Float(f_val), _) => {
                        let f: extern "C" fn(f64) -> u64 = std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(*f_val))
                    }
                    (RawArg::Int(i_val), FfiType::F64 | FfiType::F32) => {
                        let f: extern "C" fn(u64) -> f64 = std::mem::transmute(fn_ptr);
                        Ok(Value::Float(f(*i_val)))
                    }
                    _ => self.invoke_int(raw_args, fn_ptr, span),
                },
                2 => match (&raw_args[0], &raw_args[1], &self.return_type) {
                    (RawArg::Float(f1), RawArg::Float(f2), FfiType::F64 | FfiType::F32) => {
                        let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(fn_ptr);
                        Ok(Value::Float(f(*f1, *f2)))
                    }
                    (RawArg::Float(f1), RawArg::Float(f2), FfiType::Void) => {
                        let f: extern "C" fn(f64, f64) = std::mem::transmute(fn_ptr);
                        f(*f1, *f2);
                        Ok(Value::Null)
                    }
                    (RawArg::Float(f1), RawArg::Float(f2), _) => {
                        let f: extern "C" fn(f64, f64) -> u64 = std::mem::transmute(fn_ptr);
                        self.convert_int_return(f(*f1, *f2))
                    }
                    (RawArg::Float(f1), RawArg::Int(i2), FfiType::F64 | FfiType::F32) => {
                        let f: extern "C" fn(f64, u64) -> f64 = std::mem::transmute(fn_ptr);
                        Ok(Value::Float(f(*f1, *i2)))
                    }
                    (RawArg::Int(i1), RawArg::Float(f2), FfiType::F64 | FfiType::F32) => {
                        let f: extern "C" fn(u64, f64) -> f64 = std::mem::transmute(fn_ptr);
                        Ok(Value::Float(f(*i1, *f2)))
                    }
                    _ => self.invoke_int(raw_args, fn_ptr, span),
                },
                n => Err(RuntimeError::new(format!(
                    "Floating point FFI with {} arguments is not currently supported (max 2)",
                    n
                ))
                .at(span)),
            }
        }
    }

    unsafe fn convert_int_return(&self, res: u64) -> Result<Value, RuntimeError> {
        match self.return_type {
            FfiType::Void => Ok(Value::Null),
            FfiType::I32 => Ok(Value::Int(res as i32 as i64)),
            FfiType::I64 => Ok(Value::Int(res as i64)),
            FfiType::Bool => Ok(Value::Bool(res != 0)),
            FfiType::Pointer => Ok(Value::Int(res as i64)),
            FfiType::String => {
                let ptr = res as *const c_char;
                if ptr.is_null() {
                    Ok(Value::Null)
                } else {
                    let cstr = unsafe { CStr::from_ptr(ptr) };
                    Ok(Value::String(cstr.to_string_lossy().into_owned()))
                }
            }
            FfiType::F32 | FfiType::F64 => Ok(Value::Float(res as f64)),
        }
    }
}

// Memory Utilities
pub fn memory_alloc(size: usize) -> *mut u8 {
    unsafe {
        let layout = std::alloc::Layout::from_size_align(size, 8)
            .unwrap_or(std::alloc::Layout::from_size_align(1, 1).unwrap());
        std::alloc::alloc_zeroed(layout)
    }
}

pub fn memory_free(ptr: usize, size: usize) {
    if ptr != 0 {
        unsafe {
            let layout = std::alloc::Layout::from_size_align(size, 8)
                .unwrap_or(std::alloc::Layout::from_size_align(1, 1).unwrap());
            std::alloc::dealloc(ptr as *mut u8, layout);
        }
    }
}

pub fn memory_read_string(ptr: usize) -> Option<String> {
    if ptr == 0 {
        return None;
    }
    unsafe {
        let cstr = CStr::from_ptr(ptr as *const c_char);
        Some(cstr.to_string_lossy().into_owned())
    }
}

pub fn memory_read_i32(ptr: usize) -> i32 {
    if ptr == 0 {
        return 0;
    }
    unsafe { *(ptr as *const i32) }
}

pub fn memory_read_i64(ptr: usize) -> i64 {
    if ptr == 0 {
        return 0;
    }
    unsafe { *(ptr as *const i64) }
}

pub fn memory_write_i32(ptr: usize, val: i32) {
    if ptr != 0 {
        unsafe { *(ptr as *mut i32) = val };
    }
}

pub fn memory_write_i64(ptr: usize, val: i64) {
    if ptr != 0 {
        unsafe { *(ptr as *mut i64) = val };
    }
}
