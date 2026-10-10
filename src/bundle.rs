use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const BUNDLE_MAGIC: &[u8; 23] = b"SHAE_BUNDLE_PAYLOAD_V1\n";

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("Entry file not found: {0}")]
    EntryNotFound(String),
    #[error("Compilation error: {0}")]
    Compile(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleArchive {
    pub entry_path: String,
    pub files: HashMap<String, String>,
    #[serde(default)]
    pub bytecode: HashMap<String, Vec<u8>>,
    #[serde(default)]
    pub is_bytecode: bool,
}

impl BundleArchive {
    pub fn new(entry_path: &str) -> Self {
        Self {
            entry_path: entry_path.to_string(),
            files: HashMap::new(),
            bytecode: HashMap::new(),
            is_bytecode: false,
        }
    }
}

pub fn collect_bundle(entry_file: &Path) -> Result<BundleArchive, BundleError> {
    collect_bundle_opts(entry_file, true, false)
}

pub fn collect_bundle_opts(
    entry_file: &Path,
    compile_bytecode: bool,
    strip_source: bool,
) -> Result<BundleArchive, BundleError> {
    if !entry_file.exists() {
        return Err(BundleError::EntryNotFound(entry_file.display().to_string()));
    }
    if strip_source && !compile_bytecode {
        return Err(BundleError::Compile(
            "Cannot specify both --strip and --no-bytecode (bundle would be empty)".to_string(),
        ));
    }

    let canonical_entry = entry_file
        .canonicalize()
        .unwrap_or_else(|_| entry_file.to_path_buf());
    let mut archive = BundleArchive::new(&canonical_entry.to_string_lossy());
    archive.is_bytecode = compile_bytecode;

    let entry_dir = canonical_entry
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let mut queue = vec![canonical_entry];
    let mut visited = std::collections::HashSet::new();

    while let Some(current_path) = queue.pop() {
        let path_str = current_path.to_string_lossy().to_string();
        if visited.contains(&path_str) {
            continue;
        }
        visited.insert(path_str.clone());

        let raw_bytes = fs::read(&current_path)?;
        let is_precompiled = raw_bytes.starts_with(crate::chunk::BYTECODE_MAGIC)
            || current_path.extension().map_or(false, |ext| ext == "shaec");

        let (source_opt, bytecode_opt) = if is_precompiled {
            (None, Some(raw_bytes))
        } else {
            let source = String::from_utf8(raw_bytes).map_err(|e| {
                BundleError::Compile(format!("File '{}' is not valid UTF-8: {}", path_str, e))
            })?;
            let bc = if compile_bytecode {
                let bytes = crate::compile_source_to_bytecode(&source).map_err(|e| {
                    BundleError::Compile(format!("Failed to compile '{}': {}", path_str, e))
                })?;
                Some(bytes)
            } else {
                None
            };
            (Some(source), bc)
        };

        if let Some(bc) = bytecode_opt {
            archive.bytecode.insert(path_str.clone(), bc.clone());
            if let Ok(rel) = current_path.strip_prefix(&entry_dir) {
                let rel_str = rel.to_string_lossy().to_string();
                archive.bytecode.insert(rel_str.clone(), bc.clone());
                archive
                    .bytecode
                    .insert(format!("./{}", rel_str), bc.clone());
            }
        }

        if !strip_source {
            if let Some(source) = &source_opt {
                archive.files.insert(path_str.clone(), source.clone());
                if let Ok(rel) = current_path.strip_prefix(&entry_dir) {
                    let rel_str = rel.to_string_lossy().to_string();
                    archive.files.insert(rel_str.clone(), source.clone());
                    archive
                        .files
                        .insert(format!("./{}", rel_str), source.clone());
                }
            }
        }

        // Parse AST to discover local imports
        if let Some(source) = &source_opt {
            if let Ok(tokens) = crate::lexer::tokenize(source) {
                if let Ok(program) = crate::parser::parse(tokens) {
                    for stmt in &program.statements {
                        if let crate::ast::StmtKind::Use { path, .. } = &stmt.kind {
                            if path.starts_with("std:") || path.starts_with("std::") {
                                continue;
                            }

                            let parent = current_path.parent().unwrap_or(Path::new("."));
                            let base = if path.starts_with("./") || path.starts_with("../") {
                                parent.join(path)
                            } else if let Some(pkg_file) =
                                crate::pkg::resolve_package_file(parent, path)
                            {
                                pkg_file
                            } else {
                                parent.join(path)
                            };

                            let dep_path = if base.exists() {
                                base
                            } else if base.with_extension("shae").exists() {
                                base.with_extension("shae")
                            } else if base.with_extension("shaec").exists() {
                                base.with_extension("shaec")
                            } else {
                                base
                            };

                            if let Ok(canonical_dep) = dep_path.canonicalize() {
                                if canonical_dep.is_file() {
                                    queue.push(canonical_dep);
                                }
                            } else if dep_path.is_file() {
                                queue.push(dep_path);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(archive)
}

pub fn create_standalone_binary(
    entry_file: &Path,
    output_binary: &Path,
) -> Result<(), BundleError> {
    create_standalone_binary_opts(entry_file, output_binary, true, false)
}

pub fn create_standalone_binary_opts(
    entry_file: &Path,
    output_binary: &Path,
    compile_bytecode: bool,
    strip_source: bool,
) -> Result<(), BundleError> {
    let archive = collect_bundle_opts(entry_file, compile_bytecode, strip_source)?;
    let payload = serde_json::to_vec(&archive)?;
    let payload_len = payload.len() as u64;

    let current_exe = std::env::current_exe()?;
    let exe_bytes = fs::read(&current_exe)?;

    let mut out_file = fs::File::create(output_binary)?;
    use std::io::Write;
    out_file.write_all(&exe_bytes)?;
    out_file.write_all(&payload)?;
    out_file.write_all(&payload_len.to_be_bytes())?;
    out_file.write_all(BUNDLE_MAGIC)?;
    out_file.flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(output_binary)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(output_binary, perms)?;
    }

    Ok(())
}

pub fn read_embedded_bundle(exe_path: &Path) -> io::Result<Option<BundleArchive>> {
    let mut file = match fs::File::open(exe_path) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };

    let file_len = file.metadata()?.len();
    let magic_len = BUNDLE_MAGIC.len() as u64;
    let trailer_len = magic_len + 8; // 8 bytes for u64 length + magic

    if file_len < trailer_len {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-(trailer_len as i64)))?;

    let mut len_bytes = [0u8; 8];
    file.read_exact(&mut len_bytes)?;
    let payload_len = u64::from_be_bytes(len_bytes);

    let mut magic_buf = vec![0u8; magic_len as usize];
    file.read_exact(&mut magic_buf)?;

    if magic_buf != BUNDLE_MAGIC {
        return Ok(None);
    }

    if file_len < trailer_len + payload_len {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-((trailer_len + payload_len) as i64)))?;
    let mut payload = vec![0u8; payload_len as usize];
    file.read_exact(&mut payload)?;

    match serde_json::from_slice::<BundleArchive>(&payload) {
        Ok(archive) => Ok(Some(archive)),
        Err(_) => Ok(None),
    }
}
