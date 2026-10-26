use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum PkgError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Manifest parse error in '{0}': {1}")]
    ManifestParse(String, toml::de::Error),
    #[error("Manifest serialize error: {0}")]
    ManifestSerialize(#[from] toml::ser::Error),
    #[error("Lockfile parse error in '{0}': {1}")]
    LockfileParse(String, toml::de::Error),
    #[error("Git error for package '{0}': {1}")]
    Git(String, String),
    #[error("Package '{0}' not found: {1}")]
    NotFound(String, String),
    #[error("Invalid package name or manifest: {0}")]
    InvalidManifest(String),
    #[error("Package pack error: {0}")]
    PackError(String),
    #[error("Package unpack error: {0}")]
    UnpackError(String),
    #[error("Package publish error to '{0}': {1}")]
    PublishError(String, String),
    #[error("Package integrity checksum mismatch for '{0}': expected {1}, got {2}")]
    ChecksumMismatch(String, String, String),
}

pub fn validate_package_name(name: &str) -> Result<(), PkgError> {
    if name.is_empty() {
        return Err(PkgError::InvalidManifest(
            "Package name cannot be empty".into(),
        ));
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(PkgError::InvalidManifest(format!(
            "Invalid package name '{}': path traversal characters not allowed",
            name
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(PkgError::InvalidManifest(format!(
            "Invalid package name '{}': must contain only alphanumeric, '_', '-', or '.'",
            name
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencySpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub registry: Option<String>,
}

fn default_entry() -> String {
    "main.shae".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Detailed {
        git: Option<String>,
        branch: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
        path: Option<String>,
    },
}

impl DependencySpec {
    pub fn is_path(&self) -> bool {
        match self {
            DependencySpec::Simple(s) => {
                let is_git = s.starts_with("git://")
                    || s.starts_with("http://")
                    || s.starts_with("https://")
                    || s.starts_with("file://")
                    || s.starts_with("ssh://")
                    || s.starts_with("git@")
                    || s.ends_with(".git");
                !is_git && (s.starts_with('.') || s.starts_with('/') || s.starts_with('\\'))
            }
            DependencySpec::Detailed { path, git, .. } => path.is_some() && git.is_none(),
        }
    }

    pub fn get_path(&self) -> Option<String> {
        match self {
            DependencySpec::Simple(s) if self.is_path() => Some(s.clone()),
            DependencySpec::Detailed { path, .. } => path.clone(),
            _ => None,
        }
    }

    pub fn get_git_url(&self) -> Option<String> {
        match self {
            DependencySpec::Simple(s) if !self.is_path() => Some(s.clone()),
            DependencySpec::Detailed { git, .. } => git.clone(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(default = "default_lock_version")]
    pub version: u32,
    #[serde(default)]
    pub packages: BTreeMap<String, LockedPackage>,
}

fn default_lock_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    pub version: Option<String>,
    pub source: String,
    pub commit: Option<String>,
    #[serde(default)]
    pub checksum: Option<String>,
}

impl PackageManifest {
    pub fn new(name: &str) -> Self {
        Self {
            package: PackageInfo {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                entry: "main.shae".to_string(),
                description: None,
                authors: Vec::new(),
                license: None,
                repository: None,
                registry: None,
            },
            dependencies: BTreeMap::new(),
        }
    }

    pub fn load_from_dir(dir: &Path) -> Result<Self, PkgError> {
        let manifest_path = dir.join("shae.toml");
        Self::load_from_file(&manifest_path)
    }

    pub fn load_from_file(path: &Path) -> Result<Self, PkgError> {
        let content = fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| PkgError::ManifestParse(path.display().to_string(), e))
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<(), PkgError> {
        let manifest_path = dir.join("shae.toml");
        let content = toml::to_string_pretty(self)?;
        fs::write(manifest_path, content)?;
        Ok(())
    }
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            version: 1,
            packages: BTreeMap::new(),
        }
    }

    pub fn load_from_dir(dir: &Path) -> Result<Option<Self>, PkgError> {
        let lock_path = dir.join("shae.lock");
        if !lock_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&lock_path)?;
        let lock: Self = toml::from_str(&content)
            .map_err(|e| PkgError::LockfileParse(lock_path.display().to_string(), e))?;
        Ok(Some(lock))
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<(), PkgError> {
        let lock_path = dir.join("shae.lock");
        let content = toml::to_string_pretty(self)?;
        fs::write(lock_path, content)?;
        Ok(())
    }
}

pub fn init_project(dir: &Path, name_override: Option<&str>) -> Result<PackageManifest, PkgError> {
    fs::create_dir_all(dir)?;
    let name = name_override
        .map(|s| s.to_string())
        .or_else(|| {
            dir.canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        })
        .unwrap_or_else(|| "shae_project".to_string());

    let manifest = PackageManifest::new(&name);
    manifest.save_to_dir(dir)?;

    let main_file = dir.join(&manifest.package.entry);
    if !main_file.exists() {
        let template = format!(
            r#"// Welcome to {}!

fn main() {{
    print("Hello from {}!")
}}

main()
"#,
            name, name
        );
        fs::write(&main_file, template)?;
    }

    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        let gitignore_content = ".shae/\ntarget/\n";
        fs::write(gitignore, gitignore_content)?;
    }

    Ok(manifest)
}

pub fn add_dependency(
    dir: &Path,
    dep_name: &str,
    source: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    rev: Option<&str>,
) -> Result<Lockfile, PkgError> {
    validate_package_name(dep_name)?;

    let manifest_path = dir.join("shae.toml");
    let mut manifest = if manifest_path.exists() {
        PackageManifest::load_from_dir(dir)?
    } else {
        init_project(dir, None)?
    };

    let is_git_url = source.starts_with("git://")
        || source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("file://")
        || source.starts_with("ssh://")
        || source.starts_with("git@")
        || source.ends_with(".git");

    let spec = if !is_git_url
        && (source.starts_with('.') || source.starts_with('/') || source.starts_with('\\'))
    {
        DependencySpec::Detailed {
            path: Some(source.to_string()),
            git: None,
            branch: None,
            tag: None,
            rev: None,
        }
    } else if branch.is_some() || tag.is_some() || rev.is_some() {
        DependencySpec::Detailed {
            git: Some(source.to_string()),
            branch: branch.map(|s| s.to_string()),
            tag: tag.map(|s| s.to_string()),
            rev: rev.map(|s| s.to_string()),
            path: None,
        }
    } else {
        DependencySpec::Simple(source.to_string())
    };

    manifest.dependencies.insert(dep_name.to_string(), spec);
    manifest.save_to_dir(dir)?;

    install_dependencies(dir)
}

pub fn install_dependencies(dir: &Path) -> Result<Lockfile, PkgError> {
    let manifest = PackageManifest::load_from_dir(dir)?;
    let mut lockfile = Lockfile::load_from_dir(dir)?.unwrap_or_else(Lockfile::new);

    let packages_dir = dir.join(".shae").join("packages");
    fs::create_dir_all(&packages_dir)?;

    for (dep_name, spec) in &manifest.dependencies {
        validate_package_name(dep_name)?;
        let pkg_dir = packages_dir.join(dep_name);
        if !pkg_dir.starts_with(&packages_dir) {
            return Err(PkgError::InvalidManifest(format!(
                "Dependency name '{}' escapes packages directory",
                dep_name
            )));
        }

        if let Some(path_str) = spec.get_path() {
            let target_path = if Path::new(&path_str).is_absolute() {
                PathBuf::from(&path_str)
            } else {
                dir.join(&path_str)
            };

            if !target_path.exists() {
                return Err(PkgError::NotFound(
                    dep_name.clone(),
                    format!("Path '{}' does not exist", target_path.display()),
                ));
            }

            sync_path_dependency(&target_path, &pkg_dir, &packages_dir)?;

            lockfile.packages.insert(
                dep_name.clone(),
                LockedPackage {
                    version: None,
                    source: format!("path:{}", path_str),
                    commit: None,
                    checksum: None,
                },
            );
        } else if let Some(git_url) = spec.get_git_url() {
            let locked_commit = lockfile
                .packages
                .get(dep_name)
                .and_then(|lp| lp.commit.as_deref());

            let (branch, tag, rev) = match spec {
                DependencySpec::Detailed {
                    branch, tag, rev, ..
                } => (branch.as_deref(), tag.as_deref(), rev.as_deref()),
                _ => (None, None, None),
            };

            let commit_sha = fetch_git_dependency(
                &pkg_dir,
                &git_url,
                locked_commit,
                branch,
                tag,
                rev,
                dep_name,
            )?;

            lockfile.packages.insert(
                dep_name.clone(),
                LockedPackage {
                    version: None,
                    source: git_url,
                    commit: Some(commit_sha),
                    checksum: None,
                },
            );
        }
    }

    lockfile.save_to_dir(dir)?;
    Ok(lockfile)
}

fn sync_path_dependency(source: &Path, dest: &Path, packages_dir: &Path) -> Result<(), PkgError> {
    if !dest.starts_with(packages_dir) {
        return Err(PkgError::InvalidManifest(format!(
            "Destination '{}' escapes packages directory '{}'",
            dest.display(),
            packages_dir.display()
        )));
    }
    if dest.exists() {
        let _ = fs::remove_dir_all(dest);
    }
    copy_dir_recursive(source, dest)?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_name = entry.file_name().to_string_lossy().to_string();

        if file_name == ".git" || file_name == "target" || file_name == ".shae" {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn fetch_git_dependency(
    pkg_dir: &Path,
    git_url: &str,
    locked_commit: Option<&str>,
    branch: Option<&str>,
    tag: Option<&str>,
    rev: Option<&str>,
    dep_name: &str,
) -> Result<String, PkgError> {
    if !pkg_dir.exists() {
        let output = Command::new("git")
            .args(["clone", git_url, &pkg_dir.to_string_lossy()])
            .output()
            .map_err(|e| PkgError::Git(dep_name.to_string(), e.to_string()))?;

        if !output.status.success() {
            return Err(PkgError::Git(
                dep_name.to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
    } else {
        let _ = Command::new("git")
            .current_dir(pkg_dir)
            .args(["fetch", "--all"])
            .output();
    }

    let checkout_target = if let Some(commit) = locked_commit {
        commit
    } else if let Some(r) = rev {
        r
    } else if let Some(t) = tag {
        t
    } else if let Some(b) = branch {
        b
    } else {
        "HEAD"
    };

    if checkout_target != "HEAD" {
        let checkout_out = Command::new("git")
            .current_dir(pkg_dir)
            .args(["checkout", checkout_target])
            .output()
            .map_err(|e| PkgError::Git(dep_name.to_string(), e.to_string()))?;

        if !checkout_out.status.success() {
            return Err(PkgError::Git(
                dep_name.to_string(),
                format!(
                    "Failed to checkout {}: {}",
                    checkout_target,
                    String::from_utf8_lossy(&checkout_out.stderr)
                ),
            ));
        }
    }

    let rev_parse = Command::new("git")
        .current_dir(pkg_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| PkgError::Git(dep_name.to_string(), e.to_string()))?;

    if !rev_parse.status.success() {
        return Err(PkgError::Git(
            dep_name.to_string(),
            String::from_utf8_lossy(&rev_parse.stderr).to_string(),
        ));
    }

    let commit_sha = String::from_utf8_lossy(&rev_parse.stdout)
        .trim()
        .to_string();
    Ok(commit_sha)
}

pub fn resolve_package_file(search_dir: &Path, import_path: &str) -> Option<PathBuf> {
    let (pkg_name, subpath) = if let Some((p, s)) = import_path.split_once('/') {
        (p, Some(s))
    } else {
        (import_path, None)
    };

    // Ascend upwards looking for .shae/packages/<pkg_name>
    let mut current = if search_dir.is_file() {
        search_dir.parent()?.to_path_buf()
    } else {
        search_dir.to_path_buf()
    };

    loop {
        let pkg_dir = current.join(".shae").join("packages").join(pkg_name);
        if pkg_dir.is_dir() {
            if let Some(sub) = subpath {
                let candidate = pkg_dir.join(sub);
                if candidate.is_file() {
                    return Some(candidate);
                }
                let with_ext = pkg_dir.join(format!("{}.shae", sub));
                if with_ext.is_file() {
                    return Some(with_ext);
                }
                let in_src = pkg_dir.join("src").join(format!("{}.shae", sub));
                if in_src.is_file() {
                    return Some(in_src);
                }
            } else {
                // Read package entry from manifest if available
                if let Ok(manifest) = PackageManifest::load_from_dir(&pkg_dir) {
                    let entry_candidate = pkg_dir.join(&manifest.package.entry);
                    if entry_candidate.is_file() {
                        return Some(entry_candidate);
                    }
                }
                for default_entry in &["main.shae", "src/main.shae", "lib.shae", "index.shae"] {
                    let candidate = pkg_dir.join(default_entry);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }

        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    None
}

// ===========================================================================
// Milestone 5: Packaging, Publishing & Integrity Verification
// ===========================================================================

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use tar::{Archive, Builder};

/// Computes hex-encoded SHA-256 checksum for arbitrary binary data.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    context.update(data);
    let digest = context.finish();
    digest
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Result metadata from packaging a Shae project archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackResult {
    pub package_name: String,
    pub version: String,
    pub archive_path: PathBuf,
    pub file_count: usize,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Result metadata from publishing a Shae package archive to a registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublishResult {
    pub package_name: String,
    pub version: String,
    pub sha256: String,
    pub registry_url: String,
    pub success: bool,
    pub message: String,
}

fn should_skip_pack_path(name: &str) -> bool {
    name == "target"
        || name == ".git"
        || name == ".packages"
        || name == "node_modules"
        || name == "doc"
        || name.ends_with(".shaepkg")
        || name.ends_with(".tar.gz")
        || name.ends_with(".lock")
}

fn collect_pack_files_recursive(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PkgError> {
    if !current_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        if should_skip_pack_path(&file_name) {
            continue;
        }

        if path.is_dir() {
            collect_pack_files_recursive(base_dir, &path, files)?;
        } else if path.is_file() {
            let rel_path = path
                .strip_prefix(base_dir)
                .map_err(|e| PkgError::PackError(format!("Path prefix error: {}", e)))?
                .to_path_buf();
            files.push((path, rel_path));
        }
    }
    Ok(())
}

/// Packs a Shae package directory into a verified `.shaepkg` archive (gzip-compressed tarball).
pub fn pack_package(dir: &Path, output_file: Option<&Path>) -> Result<PackResult, PkgError> {
    let manifest = PackageManifest::load_from_dir(dir)?;
    let entry_file = dir.join(&manifest.package.entry);
    if !entry_file.exists() {
        return Err(PkgError::PackError(format!(
            "Package entrypoint '{}' does not exist in {}",
            manifest.package.entry,
            dir.display()
        )));
    }

    let mut files = Vec::new();
    collect_pack_files_recursive(dir, dir, &mut files)?;

    if files.is_empty() {
        return Err(PkgError::PackError(format!(
            "No packable source files found in {}",
            dir.display()
        )));
    }

    // Sort files for reproducible deterministic archives
    files.sort_by(|a, b| a.1.cmp(&b.1));

    let enc = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = Builder::new(enc);

    let prefix = format!("{}-{}", manifest.package.name, manifest.package.version);
    let file_count = files.len();

    for (abs_path, rel_path) in files {
        let archive_entry_path = Path::new(&prefix).join(&rel_path);
        let mut f = fs::File::open(&abs_path)?;
        tar.append_file(&archive_entry_path, &mut f).map_err(|e| {
            PkgError::PackError(format!("Failed to archive {}: {}", rel_path.display(), e))
        })?;
    }

    let enc = tar
        .into_inner()
        .map_err(|e| PkgError::PackError(format!("Failed to finish tar archive: {}", e)))?;
    let archive_bytes = enc
        .finish()
        .map_err(|e| PkgError::PackError(format!("Failed to gzip tarball: {}", e)))?;

    let sha256 = compute_sha256(&archive_bytes);
    let size_bytes = archive_bytes.len() as u64;

    let out_path = match output_file {
        Some(p) => p.to_path_buf(),
        None => dir.join(format!(
            "{}-{}.shaepkg",
            manifest.package.name, manifest.package.version
        )),
    };

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(&out_path, &archive_bytes)?;

    Ok(PackResult {
        package_name: manifest.package.name,
        version: manifest.package.version,
        archive_path: out_path,
        file_count,
        size_bytes,
        sha256,
    })
}

/// Unpacks a `.shaepkg` archive into a target directory.
pub fn unpack_package(archive_path: &Path, dest_dir: &Path) -> Result<PathBuf, PkgError> {
    let bytes = fs::read(archive_path)
        .map_err(|e| PkgError::UnpackError(format!("Failed to read archive: {}", e)))?;
    let decoder = GzDecoder::new(&bytes[..]);
    let mut archive = Archive::new(decoder);

    fs::create_dir_all(dest_dir)?;

    let mut root_dir: Option<PathBuf> = None;

    for entry in archive
        .entries()
        .map_err(|e| PkgError::UnpackError(format!("Corrupt archive entries: {}", e)))?
    {
        let mut entry =
            entry.map_err(|e| PkgError::UnpackError(format!("Corrupt archive entry: {}", e)))?;
        let entry_path = entry
            .path()
            .map_err(|e| PkgError::UnpackError(format!("Invalid path in archive: {}", e)))?
            .to_path_buf();

        // Prevent path traversal
        if entry_path.is_absolute()
            || entry_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(PkgError::UnpackError(format!(
                "Path traversal detected in archive entry: {}",
                entry_path.display()
            )));
        }

        if root_dir.is_none() {
            if let Some(first_comp) = entry_path.components().next() {
                root_dir = Some(PathBuf::from(first_comp.as_os_str()));
            }
        }

        let target_file = dest_dir.join(&entry_path);
        if let Some(parent) = target_file.parent() {
            fs::create_dir_all(parent)?;
        }
        entry
            .unpack(&target_file)
            .map_err(|e| PkgError::UnpackError(format!("Failed to extract file: {}", e)))?;
    }

    Ok(dest_dir.to_path_buf())
}

/// Verifies that an archive file matches the expected SHA-256 hash.
pub fn verify_package_checksum(
    archive_path: &Path,
    expected_sha256: &str,
) -> Result<bool, PkgError> {
    let bytes = fs::read(archive_path).map_err(|e| PkgError::Io(e))?;
    let actual_sha256 = compute_sha256(&bytes);
    if actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        Ok(true)
    } else {
        Err(PkgError::ChecksumMismatch(
            archive_path.to_string_lossy().to_string(),
            expected_sha256.to_string(),
            actual_sha256,
        ))
    }
}

/// Publishes a Shae package archive to a package registry (or local registry directory / mock).
pub fn publish_package(
    dir: &Path,
    registry_url: Option<&str>,
    auth_token: Option<&str>,
    dry_run: bool,
) -> Result<PublishResult, PkgError> {
    let pack_res = pack_package(dir, None)?;

    if dry_run {
        return Ok(PublishResult {
            package_name: pack_res.package_name.clone(),
            version: pack_res.version.clone(),
            sha256: pack_res.sha256.clone(),
            registry_url: "dry-run".to_string(),
            success: true,
            message: format!(
                "Dry run: verified and packed '{}' v{} ({} bytes, {} files, sha256: {})",
                pack_res.package_name,
                pack_res.version,
                pack_res.size_bytes,
                pack_res.file_count,
                pack_res.sha256
            ),
        });
    }

    let manifest = PackageManifest::load_from_dir(dir)?;
    let target_url = registry_url
        .map(|s| s.to_string())
        .or_else(|| std::env::var("SHAE_REGISTRY_URL").ok())
        .or_else(|| manifest.package.registry)
        .unwrap_or_else(|| "https://pkg.shae-lang.org/api/v1/publish".to_string());

    let token = auth_token
        .map(|s| s.to_string())
        .or_else(|| std::env::var("SHAE_REGISTRY_TOKEN").ok());

    // 1. Local filesystem registry support (file:// or raw directory path)
    if target_url.starts_with("file://")
        || Path::new(&target_url).is_dir()
        || target_url.starts_with("./")
        || target_url.starts_with("/")
    {
        let local_dir = if let Some(stripped) = target_url.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(&target_url)
        };
        let pkg_dest_dir = local_dir.join(&pack_res.package_name);
        fs::create_dir_all(&pkg_dest_dir)?;

        let dest_archive = pkg_dest_dir.join(format!(
            "{}-{}.shaepkg",
            pack_res.package_name, pack_res.version
        ));
        fs::copy(&pack_res.archive_path, &dest_archive)?;

        // Update or write local index.json
        let index_file = local_dir.join("index.json");
        let mut index_data: serde_json::Value = if index_file.exists() {
            fs::read_to_string(&index_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if let Some(obj) = index_data.as_object_mut() {
            let pkg_entries = obj
                .entry(pack_res.package_name.clone())
                .or_insert_with(|| serde_json::json!({}));
            if let Some(pkg_map) = pkg_entries.as_object_mut() {
                pkg_map.insert(
                    pack_res.version.clone(),
                    serde_json::json!({
                        "version": pack_res.version,
                        "sha256": pack_res.sha256,
                        "size": pack_res.size_bytes,
                        "files": pack_res.file_count,
                        "archive": dest_archive.to_string_lossy(),
                    }),
                );
            }
        }

        let _ = fs::write(
            &index_file,
            serde_json::to_string_pretty(&index_data).unwrap_or_default(),
        );
        let success_message = format!(
            "Published to local registry directory at {}",
            local_dir.display()
        );

        return Ok(PublishResult {
            package_name: pack_res.package_name,
            version: pack_res.version,
            sha256: pack_res.sha256,
            registry_url: target_url,
            success: true,
            message: success_message,
        });
    }

    // 2. HTTP(S) remote registry upload
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| PkgError::PublishError(target_url.clone(), e.to_string()))?;

    let archive_bytes = fs::read(&pack_res.archive_path).map_err(|e| {
        PkgError::PublishError(target_url.clone(), format!("Failed to read archive: {}", e))
    })?;

    let mut request = client
        .post(&target_url)
        .header("Content-Type", "application/octet-stream")
        .header("X-Shae-Package-Name", &pack_res.package_name)
        .header("X-Shae-Package-Version", &pack_res.version)
        .header("X-Shae-Package-Checksum", &pack_res.sha256);

    if let Some(tok) = token {
        request = request.header("Authorization", format!("Bearer {}", tok));
    }

    let response = request
        .body(archive_bytes)
        .send()
        .map_err(|e| PkgError::PublishError(target_url.clone(), e.to_string()))?;

    let status = response.status();
    if status.is_success() {
        Ok(PublishResult {
            package_name: pack_res.package_name,
            version: pack_res.version,
            sha256: pack_res.sha256,
            registry_url: target_url,
            success: true,
            message: format!("HTTP {}", status),
        })
    } else {
        let err_body = response.text().unwrap_or_default();
        Err(PkgError::PublishError(
            target_url,
            format!("Registry returned HTTP {}: {}", status, err_body),
        ))
    }
}
