use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::{Archive, Builder};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SHAEPKG_MAGIC: &[u8] = b"SHAEPKG1";

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
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Registry error: {0}")]
    Registry(String),
    #[error("Checksum mismatch for package '{0}': expected {1}, computed {2}")]
    ChecksumMismatch(String, String, String),
    #[error("Archive error: {0}")]
    Archive(String),
    #[error("Package pack error: {0}")]
    PackError(String),
    #[error("Package unpack error: {0}")]
    UnpackError(String),
    #[error("Package publish error to {0}: {1}")]
    PublishError(String, String),
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

pub fn compute_sha256(bytes: &[u8]) -> String {
    let d = digest(&SHA256, bytes);
    let mut out = String::with_capacity(64);
    for b in d.as_ref() {
        let _ = write!(out, "{:02x}", b);
    }
    out
}

pub fn verify_checksum(bytes: &[u8], expected_hex: &str, pkg_name: &str) -> Result<(), PkgError> {
    let computed = compute_sha256(bytes);
    if computed.to_lowercase() != expected_hex.trim().to_lowercase() {
        return Err(PkgError::ChecksumMismatch(
            pkg_name.to_string(),
            expected_hex.trim().to_string(),
            computed,
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RegistryConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryConfig>,
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
}

fn default_entry() -> String {
    "main.shae".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Detailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rev: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

    pub fn is_git(&self) -> bool {
        match self {
            DependencySpec::Simple(s) => {
                s.starts_with("git://")
                    || s.starts_with("http://")
                    || s.starts_with("https://")
                    || s.starts_with("file://")
                    || s.starts_with("ssh://")
                    || s.starts_with("git@")
                    || s.ends_with(".git")
            }
            DependencySpec::Detailed { git, .. } => git.is_some(),
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
            DependencySpec::Simple(s) if self.is_git() => Some(s.clone()),
            DependencySpec::Detailed { git, .. } => git.clone(),
            _ => None,
        }
    }

    pub fn get_version(&self) -> Option<String> {
        match self {
            DependencySpec::Simple(s) if !self.is_path() && !self.is_git() => Some(s.clone()),
            DependencySpec::Detailed { version, .. } => version.clone(),
            _ => None,
        }
    }

    pub fn get_registry_url(&self) -> Option<String> {
        match self {
            DependencySpec::Detailed { registry, .. } => registry.clone(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
            },
            registry: None,
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
        let gitignore_content = ".shae/\n.packages/\ntarget/\n";
        fs::write(gitignore, gitignore_content)?;
    }

    Ok(manifest)
}

pub fn resolve_registry_url(dir: Option<&Path>, cli_registry: Option<&str>) -> String {
    if let Some(url) = cli_registry {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.trim_end_matches('/').to_string();
        }
    }
    if let Ok(env_url) = std::env::var("SHAE_REGISTRY_URL") {
        let trimmed = env_url.trim();
        if !trimmed.is_empty() {
            return trimmed.trim_end_matches('/').to_string();
        }
    }
    if let Some(d) = dir {
        if let Ok(manifest) = PackageManifest::load_from_dir(d) {
            if let Some(reg) = manifest.registry {
                if let Some(def) = reg.default {
                    let trimmed = def.trim();
                    if !trimmed.is_empty() {
                        return trimmed.trim_end_matches('/').to_string();
                    }
                }
            }
        }
    }
    "https://registry.shae-lang.org".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageIndexVersion {
    pub version: String,
    pub download_url: String,
    pub checksum: String,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageIndexEntry {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub latest: Option<String>,
    #[serde(default)]
    pub versions: BTreeMap<String, PackageIndexVersion>,
}

impl PackageIndexEntry {
    pub fn get_version(&self, req: Option<&str>) -> Result<&PackageIndexVersion, PkgError> {
        if self.versions.is_empty() {
            return Err(PkgError::NotFound(
                self.name.clone(),
                "Package index has no versions available".into(),
            ));
        }

        if let Some(r) = req {
            let clean_req = r.trim().trim_start_matches('v').trim_start_matches('^');
            if let Some(v) = self.versions.get(r) {
                return Ok(v);
            }
            if let Some(v) = self.versions.get(clean_req) {
                return Ok(v);
            }
            for (ver_str, info) in &self.versions {
                if ver_str == clean_req || ver_str.starts_with(clean_req) {
                    return Ok(info);
                }
            }
            Err(PkgError::NotFound(
                self.name.clone(),
                format!("Version '{}' not found for package '{}'", r, self.name),
            ))
        } else {
            if let Some(ref lat) = self.latest {
                if let Some(v) = self.versions.get(lat) {
                    return Ok(v);
                }
            }
            self.versions
                .values()
                .last()
                .ok_or_else(|| PkgError::NotFound(self.name.clone(), "No versions available".into()))
        }
    }
}

pub fn fetch_package_index(registry_url: &str, pkg_name: &str) -> Result<PackageIndexEntry, PkgError> {
    validate_package_name(pkg_name)?;
    let base = registry_url.trim().trim_end_matches('/');
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| PkgError::Http(e.to_string()))?;

    let url1 = format!("{}/packages/{}.json", base, pkg_name);
    let resp1 = client.get(&url1).send();
    let body = match resp1 {
        Ok(r) if r.status().is_success() => {
            r.text().map_err(|e| PkgError::Http(e.to_string()))?
        }
        _ => {
            let url2 = format!("{}/index/{}.json", base, pkg_name);
            let r2 = client
                .get(&url2)
                .send()
                .map_err(|e| PkgError::Http(format!("Failed to connect to registry: {}", e)))?;
            if !r2.status().is_success() {
                return Err(PkgError::NotFound(
                    pkg_name.to_string(),
                    format!("Package not found on registry at {} or {}", url1, url2),
                ));
            }
            r2.text().map_err(|e| PkgError::Http(e.to_string()))?
        }
    };

    serde_json::from_str(&body)
        .map_err(|e| PkgError::Registry(format!("Failed to parse index JSON for '{}': {}", pkg_name, e)))
}

pub fn download_package_archive(download_url: &str, registry_url: &str) -> Result<Vec<u8>, PkgError> {
    let url = if download_url.starts_with("http://") || download_url.starts_with("https://") {
        download_url.to_string()
    } else {
        let base = registry_url.trim().trim_end_matches('/');
        let path = download_url.trim_start_matches('/');
        format!("{}/{}", base, path)
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| PkgError::Http(e.to_string()))?;

    let resp = client
        .get(&url)
        .send()
        .map_err(|e| PkgError::Http(format!("Failed to download {}: {}", url, e)))?;

    if !resp.status().is_success() {
        return Err(PkgError::Http(format!(
            "Failed to download archive from {}: HTTP status {}",
            url,
            resp.status()
        )));
    }

    let bytes = resp.bytes().map_err(|e| PkgError::Http(e.to_string()))?.to_vec();
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaepkgFile {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub is_binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShaepkgFiles {
    Map(BTreeMap<String, String>),
    List(Vec<ShaepkgFile>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaepkgArchive {
    pub name: String,
    pub version: String,
    pub files: ShaepkgFiles,
}

pub fn validate_archive_path(base_dir: &Path, rel_path: &str) -> Result<PathBuf, PkgError> {
    let clean = rel_path.trim();
    if clean.is_empty() {
        return Err(PkgError::Archive("Empty file path in archive".into()));
    }
    if clean.starts_with('/') || clean.starts_with('\\') || clean.starts_with('~') {
        return Err(PkgError::Archive(format!(
            "Path traversal attack: absolute or home paths forbidden in archive: '{}'",
            rel_path
        )));
    }
    let p = Path::new(clean);
    if p.is_absolute() {
        return Err(PkgError::Archive(format!(
            "Path traversal attack: absolute paths forbidden in archive: '{}'",
            rel_path
        )));
    }
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                return Err(PkgError::Archive(format!(
                    "Path traversal attack: '..' component detected in archive path: '{}'",
                    rel_path
                )));
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(PkgError::Archive(format!(
                    "Path traversal attack: root or prefix component forbidden: '{}'",
                    rel_path
                )));
            }
            std::path::Component::Normal(n) => {
                let s = n.to_string_lossy();
                if s == ".." || s == "." {
                    return Err(PkgError::Archive(format!(
                        "Path traversal attack: invalid path segment '{}' in: '{}'",
                        s, rel_path
                    )));
                }
            }
            _ => {}
        }
    }
    let dest = base_dir.join(p);
    if !dest.starts_with(base_dir) {
        return Err(PkgError::Archive(format!(
            "Path traversal attack: path '{}' escapes target directory '{}'",
            rel_path,
            base_dir.display()
        )));
    }
    Ok(dest)
}

pub fn extract_shaepkg(bytes: &[u8], target_dir: &Path) -> Result<(), PkgError> {
    if bytes.starts_with(SHAEPKG_MAGIC) {
        let payload = &bytes[SHAEPKG_MAGIC.len()..];
        extract_json_archive(payload, target_dir)
    } else if bytes.starts_with(b"{") {
        extract_json_archive(bytes, target_dir)
    } else if is_tar_header(bytes) {
        extract_tar(bytes, target_dir)
    } else {
        Err(PkgError::Archive(
            "Unrecognized package archive format (expected SHAEPKG1 magic, JSON envelope, or tar)".into(),
        ))
    }
}

fn extract_json_archive(payload: &[u8], target_dir: &Path) -> Result<(), PkgError> {
    let archive: ShaepkgArchive = serde_json::from_slice(payload)
        .map_err(|e| PkgError::Archive(format!("Invalid archive JSON: {}", e)))?;

    fs::create_dir_all(target_dir)?;

    match archive.files {
        ShaepkgFiles::Map(map) => {
            for (rel_path, content) in map {
                let dest = validate_archive_path(target_dir, &rel_path)?;
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&dest, content)?;
            }
        }
        ShaepkgFiles::List(list) => {
            for item in list {
                let dest = validate_archive_path(target_dir, &item.path)?;
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                if item.is_binary {
                    let decoded = BASE64_STANDARD
                        .decode(item.content.trim())
                        .map_err(|e| PkgError::Archive(format!("Base64 decode error for '{}': {}", item.path, e)))?;
                    fs::write(&dest, decoded)?;
                } else {
                    fs::write(&dest, item.content)?;
                }
            }
        }
    }
    Ok(())
}

fn is_tar_header(bytes: &[u8]) -> bool {
    if bytes.len() >= 512 {
        let magic = &bytes[257..262];
        magic == b"ustar"
    } else {
        false
    }
}

pub fn extract_tar(bytes: &[u8], target_dir: &Path) -> Result<(), PkgError> {
    let mut offset = 0;
    fs::create_dir_all(target_dir)?;

    while offset + 512 <= bytes.len() {
        let block = &bytes[offset..offset + 512];
        offset += 512;

        if block.iter().all(|&b| b == 0) {
            break;
        }

        let raw_name = &block[0..100];
        let name_end = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
        let name = String::from_utf8_lossy(&raw_name[..name_end]).trim().to_string();
        if name.is_empty() {
            continue;
        }

        let raw_size = &block[124..136];
        let size_str = String::from_utf8_lossy(raw_size);
        let size = u64::from_str_radix(size_str.trim().trim_matches('\0').trim(), 8).unwrap_or(0) as usize;

        let typeflag = block[156];

        if offset + size > bytes.len() {
            return Err(PkgError::Archive("Truncated tar archive".into()));
        }

        let file_data = &bytes[offset..offset + size];
        let padding = (512 - (size % 512)) % 512;
        offset += size + padding;

        let dest = validate_archive_path(target_dir, &name)?;

        if typeflag == b'5' || name.ends_with('/') {
            fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, file_data)?;
        }
    }
    Ok(())
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
    collected: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PkgError> {
    if !current_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current_dir).map_err(PkgError::Io)? {
        let entry = entry.map_err(PkgError::Io)?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if should_skip_pack_path(file_name) {
            continue;
        }

        if path.is_dir() {
            collect_pack_files_recursive(base_dir, &path, collected)?;
        } else if path.is_file() {
            let rel_path = path
                .strip_prefix(base_dir)
                .map_err(|e| PkgError::PackError(e.to_string()))?
                .to_path_buf();
            collected.push((path, rel_path));
        }
    }
    Ok(())
}

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

    files.sort_by(|a, b| a.1.cmp(&b.1));

    let enc = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = Builder::new(enc);

    let prefix = format!("{}-{}", manifest.package.name, manifest.package.version);
    let file_count = files.len();

    for (abs_path, rel_path) in files {
        let archive_entry_path = Path::new(&prefix).join(&rel_path);
        let mut f = fs::File::open(&abs_path).map_err(PkgError::Io)?;
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

    let target_path = match output_file {
        Some(p) => p.to_path_buf(),
        None => dir.join(format!("{}-{}.shaepkg", manifest.package.name, manifest.package.version)),
    };

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(PkgError::Io)?;
    }
    fs::write(&target_path, &archive_bytes).map_err(PkgError::Io)?;

    Ok(PackResult {
        package_name: manifest.package.name,
        version: manifest.package.version,
        archive_path: target_path,
        file_count,
        size_bytes,
        sha256,
    })
}

pub fn unpack_package(archive_path: &Path, dest_dir: &Path) -> Result<PathBuf, PkgError> {
    let bytes = fs::read(archive_path)
        .map_err(|e| PkgError::UnpackError(format!("Failed to read archive: {}", e)))?;
    let decoder = GzDecoder::new(&bytes[..]);
    let mut archive = Archive::new(decoder);

    fs::create_dir_all(dest_dir).map_err(PkgError::Io)?;

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
            fs::create_dir_all(parent).map_err(PkgError::Io)?;
        }
        entry
            .unpack(&target_file)
            .map_err(|e| PkgError::UnpackError(format!("Failed to unpack entry {}: {}", entry_path.display(), e)))?;
    }

    root_dir.ok_or_else(|| PkgError::UnpackError("Empty package archive".into()))
}

pub fn create_shaepkg(src_dir: &Path) -> Result<Vec<u8>, PkgError> {
    let manifest_path = src_dir.join("shae.toml");
    if !manifest_path.exists() {
        return Err(PkgError::NotFound(
            src_dir.display().to_string(),
            "shae.toml not found in directory to package".into(),
        ));
    }
    let manifest = PackageManifest::load_from_dir(src_dir)?;
    validate_package_name(&manifest.package.name)?;

    let mut files = BTreeMap::new();
    collect_package_files(src_dir, src_dir, &mut files)?;

    if !files.contains_key("shae.toml") {
        let content = fs::read_to_string(&manifest_path)?;
        files.insert("shae.toml".to_string(), content);
    }

    let archive = ShaepkgArchive {
        name: manifest.package.name,
        version: manifest.package.version,
        files: ShaepkgFiles::Map(files),
    };

    let json_bytes = serde_json::to_vec(&archive)
        .map_err(|e| PkgError::Archive(format!("Failed to serialize package archive: {}", e)))?;

    let mut payload = Vec::with_capacity(SHAEPKG_MAGIC.len() + json_bytes.len());
    payload.extend_from_slice(SHAEPKG_MAGIC);
    payload.extend_from_slice(&json_bytes);
    Ok(payload)
}
pub fn pack_to_file(src_dir: &Path, out_dir: &Path) -> Result<PathBuf, PkgError> {
    let manifest = PackageManifest::load_from_dir(src_dir)?;
    let bytes = create_shaepkg(src_dir)?;
    let filename = format!("{}-{}.shaepkg", manifest.package.name, manifest.package.version);
    let out_file = out_dir.join(filename);
    fs::create_dir_all(out_dir)?;
    fs::write(&out_file, bytes)?;
    Ok(out_file)
}

fn collect_package_files(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), PkgError> {
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.')
            || name == "target"
            || name == ".packages"
            || name == ".shae"
            || name.ends_with(".shaepkg")
            || name.ends_with(".lock")
        {
            continue;
        }

        if path.is_dir() {
            collect_package_files(base_dir, &path, files)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base_dir)
                .map_err(|e| PkgError::Archive(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(&path)
                .map_err(|e| PkgError::Archive(format!("File '{}' is not valid UTF-8: {}", rel, e)))?;
            files.insert(rel, content);
        }
    }
    Ok(())
}

pub fn add_dependency_from_registry(
    dir: &Path,
    name: &str,
    version_req: Option<&str>,
    registry_url: &str,
) -> Result<Lockfile, PkgError> {
    validate_package_name(name)?;

    let manifest_path = dir.join("shae.toml");
    let mut manifest = if manifest_path.exists() {
        PackageManifest::load_from_dir(dir)?
    } else {
        init_project(dir, None)?
    };

    let index_entry = fetch_package_index(registry_url, name)?;
    let ver_info = index_entry.get_version(version_req)?;

    let archive_bytes = download_package_archive(&ver_info.download_url, registry_url)?;

    // Strict SHA-256 verification before extracting
    verify_checksum(&archive_bytes, &ver_info.checksum, name)?;

    // Safe extraction into .packages/<name>/
    let packages_dir = dir.join(".packages");
    let pkg_dest = packages_dir.join(name);
    extract_shaepkg(&archive_bytes, &pkg_dest)?;

    // Update manifest dependencies
    manifest.dependencies.insert(
        name.to_string(),
        DependencySpec::Detailed {
            version: Some(ver_info.version.clone()),
            registry: Some(registry_url.to_string()),
            git: None,
            branch: None,
            tag: None,
            rev: None,
            path: None,
        },
    );
    manifest.save_to_dir(dir)?;

    // Update lockfile
    let mut lockfile = Lockfile::load_from_dir(dir)?.unwrap_or_else(Lockfile::new);
    lockfile.packages.insert(
        name.to_string(),
        LockedPackage {
            version: Some(ver_info.version.clone()),
            source: format!("registry+{}", registry_url),
            commit: None,
            checksum: Some(ver_info.checksum.clone()),
        },
    );
    lockfile.save_to_dir(dir)?;

    Ok(lockfile)
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
            version: None,
            registry: None,
            path: Some(source.to_string()),
            git: None,
            branch: None,
            tag: None,
            rev: None,
        }
    } else if branch.is_some() || tag.is_some() || rev.is_some() {
        DependencySpec::Detailed {
            version: None,
            registry: None,
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
        } else {
            // Registry dependency: install into .packages/<dep_name>
            let target_pkg_dir = dir.join(".packages").join(dep_name);
            let registry_url = spec
                .get_registry_url()
                .or_else(|| {
                    lockfile
                        .packages
                        .get(dep_name)
                        .and_then(|lp| lp.source.strip_prefix("registry+").map(|s| s.to_string()))
                })
                .unwrap_or_else(|| resolve_registry_url(Some(dir), None));

            let ver_req = spec.get_version();
            let index = fetch_package_index(&registry_url, dep_name)?;
            let ver_info = index.get_version(ver_req.as_deref())?;

            let archive_bytes = download_package_archive(&ver_info.download_url, &registry_url)?;
            verify_checksum(&archive_bytes, &ver_info.checksum, dep_name)?;
            extract_shaepkg(&archive_bytes, &target_pkg_dir)?;

            lockfile.packages.insert(
                dep_name.clone(),
                LockedPackage {
                    version: Some(ver_info.version.clone()),
                    source: format!("registry+{}", registry_url),
                    commit: None,
                    checksum: Some(ver_info.checksum.clone()),
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

        if file_name == ".git" || file_name == "target" || file_name == ".shae" || file_name == ".packages" {
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

    let mut current = if search_dir.is_file() {
        search_dir.parent()?.to_path_buf()
    } else {
        search_dir.to_path_buf()
    };

    loop {
        // Priority 1: .packages/<pkg_name>
        // Priority 2: .shae/packages/<pkg_name> (backward compatibility)
        for base in &[
            current.join(".packages").join(pkg_name),
            current.join(".shae").join("packages").join(pkg_name),
        ] {
            if base.is_dir() {
                if let Some(sub) = subpath {
                    let candidate = base.join(sub);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                    let with_ext = base.join(format!("{}.shae", sub));
                    if with_ext.is_file() {
                        return Some(with_ext);
                    }
                    let in_src = base.join("src").join(format!("{}.shae", sub));
                    if in_src.is_file() {
                        return Some(in_src);
                    }
                } else {
                    if let Ok(manifest) = PackageManifest::load_from_dir(base) {
                        let entry_candidate = base.join(&manifest.package.entry);
                        if entry_candidate.is_file() {
                            return Some(entry_candidate);
                        }
                    }
                    for default_entry in &["main.shae", "src/main.shae", "lib.shae", "index.shae"] {
                        let candidate = base.join(default_entry);
                        if candidate.is_file() {
                            return Some(candidate);
                        }
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


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PackResult {
    pub package_name: String,
    pub version: String,
    pub archive_path: PathBuf,
    pub file_count: usize,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PublishResult {
    pub package_name: String,
    pub version: String,
    pub sha256: String,
    pub registry_url: String,
    pub success: bool,
    pub message: String,
}

pub fn verify_package_checksum(
    archive_path: &Path,
    expected_sha256: &str,
) -> Result<bool, PkgError> {
    let bytes = fs::read(archive_path).map_err(PkgError::Io)?;
    verify_checksum(&bytes, expected_sha256, &archive_path.to_string_lossy())?;
    Ok(true)
}

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
        .or_else(|| manifest.registry.as_ref().and_then(|r| r.default.clone()))
        .unwrap_or_else(|| "https://shae-site.vercel.app/api/v1/publish".to_string());

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
