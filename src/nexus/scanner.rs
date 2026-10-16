use crate::error::{Result, RnuError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::PackageEntry;

/// Stateless filesystem scanner for npm packages in `node_modules`.
///
/// All methods are synchronous; no network I/O.
pub struct PackageScanner;

impl PackageScanner {
    /// Locate the `node_modules` directory from a user-supplied path.
    /// Accepts either the directory itself or a project root that contains it.
    pub fn find_node_modules(path: &Path) -> Result<PathBuf> {
        if path.file_name().and_then(|n| n.to_str()) == Some("node_modules") {
            return Ok(path.to_path_buf());
        }

        let nm = path.join("node_modules");
        if nm.exists() && nm.is_dir() {
            return Ok(nm);
        }

        Err(RnuError::PackageNotFound(format!(
            "node_modules directory not found at {} or {}",
            path.display(),
            nm.display()
        )))
    }

    /// Recursively scan `node_modules` and collect every package.
    /// Deduplicates by `name@version`.
    pub fn scan(node_modules_path: &Path) -> Result<Vec<PackageEntry>> {
        let mut packages = Vec::new();
        let mut seen = HashSet::new();
        Self::scan_dir(node_modules_path, &mut packages, &mut seen)?;
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(packages)
    }

    // ── internal helpers ──────────────────────────────────────────

    fn scan_dir(dir: &Path, out: &mut Vec<PackageEntry>, seen: &mut HashSet<String>) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| RnuError::InvalidPackage("invalid directory name".to_string()))?;

            if dir_name.starts_with('.') {
                continue;
            }

            if dir_name.starts_with('@') {
                Self::scan_dir(&path, out, seen)?;
            } else if let Ok(pkg) = Self::read_package_json(&path) {
                let key = format!("{}@{}", pkg.name, pkg.version);
                if !seen.contains(&key) {
                    seen.insert(key);
                    out.push(pkg);
                }
            }
        }
        Ok(())
    }

    fn read_package_json(dir: &Path) -> Result<PackageEntry> {
        let json_path = dir.join("package.json");
        if !json_path.exists() {
            return Err(RnuError::PackageNotFound(format!(
                "package.json not found in {}",
                dir.display()
            )));
        }

        let raw = fs::read_to_string(&json_path)?;
        let pkg: PackageJson = serde_json::from_str(&raw)?;

        let tarball_path = Self::find_tarball(dir, &pkg.name, &pkg.version);
        let name = Self::extract_name(&pkg.name);

        Ok(PackageEntry {
            name,
            version: pkg.version,
            path: dir.to_path_buf(),
            tarball_path,
        })
    }

    fn extract_name(full_name: &str) -> String {
        if let Some(pos) = full_name.rfind('/') {
            full_name[pos + 1..].to_string()
        } else if let Some(pos) = full_name.find('@') {
            full_name[..pos].to_string()
        } else {
            full_name.to_string()
        }
    }

    fn find_tarball(dir: &Path, name: &str, version: &str) -> Option<PathBuf> {
        let sanitized = name.replace('@', "").replace('/', "-");
        for candidate in &[
            format!("{}-{}.tgz", name, version),
            format!("{}-{}.tar.gz", name, version),
            format!("{}-{}.tgz", sanitized, version),
            format!("{}-{}.tar.gz", sanitized, version),
            "package.tgz".to_string(),
            "package.tar.gz".to_string(),
        ] {
            let p = dir.join(candidate);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }
}

// ── internal data types (used by scanner, owned by nexus/mod.rs) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageJson {
    pub(super) name: String,
    pub(super) version: String,
    #[serde(default)]
    pub(super) dependencies: serde_json::Map<String, Value>,
    #[serde(default, rename = "devDependencies")]
    pub(super) dev_dependencies: serde_json::Map<String, Value>,
}
