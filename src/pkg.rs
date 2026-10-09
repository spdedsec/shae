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
