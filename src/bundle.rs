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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleArchive {
    pub entry_path: String,
    pub files: HashMap<String, String>,
}

impl BundleArchive {
    pub fn new(entry_path: &str) -> Self {
        Self {
            entry_path: entry_path.to_string(),
            files: HashMap::new(),
        }
    }
}

pub fn collect_bundle(entry_file: &Path) -> Result<BundleArchive, BundleError> {
    if !entry_file.exists() {
        return Err(BundleError::EntryNotFound(entry_file.display().to_string()));
    }

    let canonical_entry = entry_file
        .canonicalize()
        .unwrap_or_else(|_| entry_file.to_path_buf());
    let mut archive = BundleArchive::new(&canonical_entry.to_string_lossy());

    let mut queue = vec![canonical_entry];
    let mut visited = std::collections::HashSet::new();

    while let Some(current_path) = queue.pop() {
        let path_str = current_path.to_string_lossy().to_string();
        if visited.contains(&path_str) {
            continue;
        }
        visited.insert(path_str.clone());

        let source = fs::read_to_string(&current_path)?;
        archive.files.insert(path_str, source.clone());

        // Parse AST to discover local imports
        if let Ok(tokens) = crate::lexer::tokenize(&source) {
            if let Ok(program) = crate::parser::parse(tokens) {
                for stmt in &program.statements {
                    if let crate::ast::StmtKind::Use { path, .. } = &stmt.kind {
                        if path.starts_with("std:") || path.starts_with("std::") {
                            continue;
                        }

                        let parent = current_path.parent().unwrap_or(Path::new("."));
                        let dep_path = if path.starts_with("./") || path.starts_with("../") {
                            parent.join(path)
                        } else if let Some(pkg_file) = crate::pkg::resolve_package_file(parent, path) {
                            pkg_file
                        } else {
                            parent.join(path)
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

    Ok(archive)
}

pub fn create_standalone_binary(entry_file: &Path, output_binary: &Path) -> Result<(), BundleError> {
    let archive = collect_bundle(entry_file)?;
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
