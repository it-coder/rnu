mod client;
mod maven;
mod scanner;

pub use client::NexusClient;
pub use maven::MavenScanner;
pub use scanner::PackageScanner;

use crate::config::Config;
use crate::error::{Result, RnuError};
use crate::utils::progress::UploadProgress;
use colored::*;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tar::Builder;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use maven::MavenArtifact;

// ── public types ──────────────────────────────────────────────────

/// A single npm package discovered in `node_modules`.
#[derive(Debug, Clone)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    /// Pre-existing tarball alongside `package.json`, if any.
    pub tarball_path: Option<PathBuf>,
}

/// Name-based include / exclude filters applied after scanning.
#[derive(Debug)]
pub struct FilterOptions {
    pub only_packages: Option<Vec<String>>,
    pub exclude_packages: Option<Vec<String>>,
}

impl FilterOptions {
    /// Returns `true` when `package_name` should be processed.
    pub fn should_include(&self, package_name: &str) -> bool {
        if let Some(ref exclude) = self.exclude_packages {
            if exclude.iter().any(|p| package_name.contains(p)) {
                return false;
            }
        }
        if let Some(ref only) = self.only_packages {
            only.iter().any(|p| package_name.contains(p))
        } else {
            true
        }
    }

    /// Convenience constructor that parses comma-separated CLI strings.
    pub fn from_cli(only: Option<&str>, exclude: Option<&str>) -> Self {
        Self {
            only_packages: only.map(|s| s.split(',').map(|t| t.trim().to_string()).collect()),
            exclude_packages: exclude.map(|s| s.split(',').map(|t| t.trim().to_string()).collect()),
        }
    }
}

// ── orchestrator ──────────────────────────────────────────────────

/// Top-level upload orchestrator.
///
/// Owns a [`NexusClient`] and a [`Config`].  Designed to be wrapped in
/// `Arc<Self>` so that the concurrent upload phase can share it across tasks.
pub struct NexusUploader {
    #[allow(dead_code)]
    config: Config,
    client: NexusClient,
}

impl NexusUploader {
    pub fn new(config: Config) -> Result<Self> {
        let client = NexusClient::new(&config)?;
        Ok(Self { config, client })
    }

    // ── public entry point ──────────────────────────────────────

    /// Execute the full npm-upload pipeline.
    ///
    /// Takes `Arc<Self>` to enable concurrent existence-checks and uploads.
    pub async fn run(
        self: Arc<Self>,
        path: PathBuf,
        skip_existing: bool,
        _include_dev: bool,
        filter_options: FilterOptions,
        concurrency: usize,
    ) -> Result<()> {
        // 1. Locate node_modules
        let nm = PackageScanner::find_node_modules(&path)?;
        info!("Scanning packages in: {}", nm.display());

        // 2. Sync scan
        let all = PackageScanner::scan(&nm)?;
        debug!("Found {} packages before filtering", all.len());
        if all.is_empty() {
            warn!("No packages found in {}", nm.display());
            return Ok(());
        }

        // 3. Apply --only / --exclude
        let candidates: Vec<PackageEntry> = all
            .into_iter()
            .filter(|p| filter_options.should_include(&p.name))
            .collect();
        if candidates.is_empty() {
            warn!("No packages match the specified filters");
            return Ok(());
        }

        // 4. Concurrent existence check (only when --skip-existing)
        let to_upload = if skip_existing {
            self.filter_existing(candidates, concurrency).await?
        } else {
            candidates
        };
        if to_upload.is_empty() {
            info!("All packages already exist on server – nothing to upload");
            return Ok(());
        }

        info!(
            "Uploading {} packages (concurrency={})",
            to_upload.len(),
            concurrency
        );

        let progress = Arc::new(Mutex::new(UploadProgress::new(to_upload.len() as u64)));

        // 5. Concurrent upload
        // 5. Upload (or dry-run preview)
        if self.config.dry_run {
            info!("Dry-run: would upload {} packages", to_upload.len());
            for pkg in &to_upload {
                println!(
                    "  {} Would upload: {}@{}",
                    "→".blue(),
                    pkg.name,
                    pkg.version
                );
                progress
                    .lock()
                    .unwrap()
                    .increment_uploaded(&pkg.name, &pkg.version);
            }
            progress.lock().unwrap().finish_with_summary();
            return Ok(());
        }

        let result = self.upload_batch(to_upload, concurrency, &progress).await;

        // Always show summary, even on partial failure.
        progress.lock().unwrap().finish_with_summary();

        result?;
        Ok(())
    }

    // ── phase helpers ───────────────────────────────────────────

    /// Concurrently query Nexus for each package; keep only new ones.
    async fn filter_existing(
        self: &Arc<Self>,
        packages: Vec<PackageEntry>,
        concurrency: usize,
    ) -> Result<Vec<PackageEntry>> {
        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let mut tasks = JoinSet::new();

        for pkg in packages {
            let permit = sem.clone().acquire_owned().await.expect("semaphore closed");
            let this = self.clone();
            tasks.spawn(async move {
                let _permit = permit;
                let exists = this
                    .client
                    .check_package_exists(&pkg.name, &pkg.version)
                    .await
                    .unwrap_or(false);
                (pkg, exists)
            });
        }

        let mut remaining = Vec::new();
        let mut skipped = 0usize;

        while let Some(Ok((pkg, exists))) = tasks.join_next().await {
            if exists {
                skipped += 1;
                debug!("  skip (exists) {}@{}", pkg.name, pkg.version);
            } else {
                remaining.push(pkg);
            }
        }

        if skipped > 0 {
            info!("Skipped {} already-existing packages", skipped);
        }
        Ok(remaining)
    }

    /// Concurrently upload every package in `packages`.
    async fn upload_batch(
        self: &Arc<Self>,
        packages: Vec<PackageEntry>,
        concurrency: usize,
        progress: &Mutex<UploadProgress>,
    ) -> Result<()> {
        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let mut tasks = JoinSet::new();

        for pkg in packages {
            let permit = sem.clone().acquire_owned().await.expect("semaphore closed");
            let this = self.clone();
            let name = pkg.name.clone();
            let version = pkg.version.clone();

            tasks.spawn(async move {
                let _permit = permit;
                let result = this.upload_single(&pkg).await;
                (name, version, result)
            });
        }

        let mut _uploaded = 0u64;
        let mut errors = 0u64;

        while let Some(task_result) = tasks.join_next().await {
            match task_result {
                Ok((name, version, Ok(()))) => {
                    println!("  {} Uploaded: {}@{}", "✓".green(), name, version);
                    _uploaded += 1;
                    progress.lock().unwrap().increment_uploaded(&name, &version);
                }
                Ok((name, version, Err(e))) => {
                    error!(%name, %version, %e, "Upload failed");
                    eprintln!(
                        "  {} Failed to upload {}@{}: {}",
                        "✗".red(),
                        name,
                        version,
                        e
                    );
                    errors += 1;
                    progress.lock().unwrap().increment_error(&name, &version);
                }
                Err(join_err) => {
                    error!("Upload task panicked: {}", join_err);
                    errors += 1;
                }
            }
        }

        if errors > 0 {
            Err(RnuError::UploadError(format!(
                "{} packages failed to upload",
                errors
            )))
        } else {
            Ok(())
        }
    }

    // ── Maven pipeline ──────────────────────────────────────────

    /// Execute the full Maven-upload pipeline.
    pub async fn run_maven(
        self: Arc<Self>,
        path: PathBuf,
        skip_existing: bool,
        filter_options: FilterOptions,
        concurrency: usize,
    ) -> Result<()> {
        let all = MavenScanner::scan(&path)?;

        let filtered: Vec<MavenArtifact> = all
            .into_iter()
            .filter(|a| filter_options.should_include(&a.artifact_id))
            .collect();
        if filtered.is_empty() {
            warn!("No artifacts match the specified filters");
            return Ok(());
        }

        let to_upload = if skip_existing {
            self.filter_existing_maven(filtered, concurrency).await?
        } else {
            filtered
        };
        if to_upload.is_empty() {
            info!("All artifacts already exist on server");
            return Ok(());
        }

        info!(
            "Uploading {} Maven artifact(s) (concurrency={})",
            to_upload.len(),
            concurrency
        );

        let progress = Arc::new(Mutex::new(UploadProgress::new(to_upload.len() as u64)));

        if self.config.dry_run {
            for a in &to_upload {
                println!(
                    "  {} Would upload: {}:{}:{}",
                    "→".blue(),
                    a.group_id,
                    a.artifact_id,
                    a.version
                );
                progress
                    .lock()
                    .unwrap()
                    .increment_uploaded(&a.artifact_id, &a.version);
            }
            progress.lock().unwrap().finish_with_summary();
            return Ok(());
        }

        let result = self
            .upload_maven_batch(to_upload, concurrency, &progress)
            .await;
        progress.lock().unwrap().finish_with_summary();
        result?;
        Ok(())
    }

    async fn filter_existing_maven(
        self: &Arc<Self>,
        artifacts: Vec<MavenArtifact>,
        concurrency: usize,
    ) -> Result<Vec<MavenArtifact>> {
        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let mut tasks = JoinSet::new();

        for a in artifacts {
            let permit = sem.clone().acquire_owned().await.expect("semaphore closed");
            let this = self.clone();
            let gid = a.group_id.clone();
            let aid = a.artifact_id.clone();
            let ver = a.version.clone();
            tasks.spawn(async move {
                let _permit = permit;
                let exists = this
                    .client
                    .check_maven_exists(&gid, &aid, &ver)
                    .await
                    .unwrap_or(false);
                (a, exists)
            });
        }

        let mut remaining = Vec::new();
        let mut skipped = 0usize;
        while let Some(Ok((a, exists))) = tasks.join_next().await {
            if exists {
                skipped += 1;
                debug!(
                    "  skip (exists) {}:{}:{}",
                    a.group_id, a.artifact_id, a.version
                );
            } else {
                remaining.push(a);
            }
        }
        if skipped > 0 {
            info!("Skipped {} already-existing artifacts", skipped);
        }
        Ok(remaining)
    }

    async fn upload_maven_batch(
        self: &Arc<Self>,
        artifacts: Vec<MavenArtifact>,
        concurrency: usize,
        progress: &Mutex<UploadProgress>,
    ) -> Result<()> {
        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let mut tasks = JoinSet::new();

        for a in artifacts {
            let permit = sem.clone().acquire_owned().await.expect("semaphore closed");
            let this = self.clone();
            let gid = a.group_id.clone();
            let aid = a.artifact_id.clone();
            let ver = a.version.clone();
            let files = a.files.clone();

            tasks.spawn(async move {
                let _permit = permit;
                let result = this
                    .client
                    .upload_maven_component(&gid, &aid, &ver, &files)
                    .await;
                (gid, aid, ver, result)
            });
        }

        let mut _uploaded = 0u64;
        let mut errors = 0u64;
        while let Some(task_result) = tasks.join_next().await {
            match task_result {
                Ok((gid, aid, ver, Ok(()))) => {
                    println!("  {} Uploaded: {}:{}:{}", "✓".green(), gid, aid, ver);
                    _uploaded += 1;
                    progress.lock().unwrap().increment_uploaded(&aid, &ver);
                }
                Ok((gid, aid, ver, Err(e))) => {
                    error!(%gid, %aid, %ver, %e, "Upload failed");
                    eprintln!("  {} Failed: {}:{}:{} - {}", "✗".red(), gid, aid, ver, e);
                    errors += 1;
                    progress.lock().unwrap().increment_error(&aid, &ver);
                }
                Err(join_err) => {
                    error!("Upload task panicked: {}", join_err);
                    errors += 1;
                }
            }
        }

        if errors > 0 {
            Err(RnuError::UploadError(format!(
                "{} artifacts failed to upload",
                errors
            )))
        } else {
            Ok(())
        }
    }

    /// Upload a single package: use its pre-existing tarball, or create one.
    async fn upload_single(&self, pkg: &PackageEntry) -> Result<()> {
        if let Some(ref tarball_path) = pkg.tarball_path {
            self.client.upload_tarball(tarball_path).await
        } else {
            self.create_and_upload_tarball(pkg).await
        }
    }

    /// Package the directory as a `.tgz` and upload it.
    async fn create_and_upload_tarball(&self, pkg: &PackageEntry) -> Result<()> {
        let tarball_name = format!("{}-{}.tgz", pkg.name.replace('/', "-"), pkg.version);
        let tarball_path = std::env::temp_dir().join(&tarball_name);

        // ---- tar.gz creation (sync) ----
        let tar_file = File::create(&tarball_path)?;
        let enc = GzEncoder::new(tar_file, Compression::default());
        let mut tar = Builder::new(enc);

        fn append_dir(
            tar: &mut Builder<impl Write>,
            base: &PathBuf,
            current: &PathBuf,
            tar_root: &str,
        ) -> io::Result<()> {
            for entry in fs::read_dir(current)? {
                let entry = entry?;
                let path = entry.path();

                let rel = path
                    .strip_prefix(base)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                let tar_path = if rel.as_os_str().is_empty() {
                    PathBuf::from(tar_root)
                } else {
                    PathBuf::from(tar_root).join(rel)
                };

                if path.is_dir() {
                    if path.file_name().and_then(|n| n.to_str()) == Some("node_modules") {
                        continue;
                    }
                    let mut hdr = tar::Header::new_gnu();
                    hdr.set_path(&tar_path)?;
                    hdr.set_size(0);
                    hdr.set_mode(0o755);
                    hdr.set_entry_type(tar::EntryType::Directory);
                    hdr.set_cksum();
                    tar.append(&hdr, &mut io::empty())?;
                    append_dir(tar, base, &path, tar_root)?;
                } else if path.is_file() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.ends_with(".log") || name == ".DS_Store" || name == "Thumbs.db" {
                            continue;
                        }
                    }
                    let mut file = File::open(&path)?;
                    let mut hdr = tar::Header::new_gnu();
                    hdr.set_path(&tar_path)?;
                    hdr.set_size(file.metadata()?.len());
                    hdr.set_mode(0o755);
                    hdr.set_cksum();
                    tar.append(&hdr, &mut file)?;
                }
            }
            Ok(())
        }

        append_dir(&mut tar, &pkg.path, &pkg.path, "package")?;
        tar.into_inner()?.finish()?;

        // ---- upload (async) ----
        let result = self.client.upload_tarball(&tarball_path).await;
        fs::remove_file(&tarball_path)?;
        result
    }

    /// Build an npm-style registry path for the given package.
    #[allow(dead_code)]
    fn build_npm_path(name: &str, version: &str) -> String {
        if name.starts_with('@') {
            let parts: Vec<&str> = name.splitn(2, '/').collect();
            if parts.len() == 2 {
                format!("{}/{}/-/{}-{}.tgz", parts[0], parts[1], parts[1], version)
            } else {
                format!(
                    "{}/-/{}-{}.tgz",
                    name,
                    name.trim_start_matches('@'),
                    version
                )
            }
        } else {
            format!("{}/-/{}-{}.tgz", name, name, version)
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::Path;
    use std::str::FromStr;
    use tempfile::tempdir;

    fn test_config() -> Config {
        Config::new(
            "http://20.21.1.180:18081".into(),
            "admin".into(),
            "Zyjc@2020".into(),
            "npm-hosted".into(),
            true,
            false,
        )
    }

    #[test]
    fn test_build_npm_path() {
        assert_eq!(
            NexusUploader::build_npm_path("lodash", "4.17.21"),
            "lodash/-/lodash-4.17.21.tgz"
        );
        assert_eq!(
            NexusUploader::build_npm_path("@types/node", "18.0.0"),
            "@types/node/-/node-18.0.0.tgz"
        );
    }

    #[test]
    fn test_filter_options() {
        let f = FilterOptions {
            only_packages: Some(vec!["lodash".into(), "express".into()]),
            exclude_packages: None,
        };
        assert!(f.should_include("lodash"));
        assert!(f.should_include("express"));
        assert!(!f.should_include("react"));

        let f = FilterOptions {
            only_packages: None,
            exclude_packages: Some(vec!["test".into()]),
        };
        assert!(f.should_include("lodash"));
        assert!(!f.should_include("test-package"));
    }

    #[test]
    fn test_filter_options_from_cli() {
        let f = FilterOptions::from_cli(Some("lodash,express"), None);
        assert!(f.should_include("lodash"));
        assert!(!f.should_include("react"));

        let f = FilterOptions::from_cli(None, Some("test,mock"));
        assert!(!f.should_include("test-utils"));
    }

    #[tokio::test]
    async fn test_create_tarball() {
        let uploader = NexusUploader::new(test_config()).unwrap();
        let dir = tempdir().unwrap();
        let pkg_dir = dir.path().join("test-pkg");
        fs::create_dir(&pkg_dir).unwrap();
        fs::write(
            pkg_dir.join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(pkg_dir.join("README.md"), "hello").unwrap();

        let entry = PackageEntry {
            name: "test".into(),
            version: "1.0.0".into(),
            path: pkg_dir,
            tarball_path: None,
        };

        // Upload will fail without a real server, but tarball creation succeeds.
        let _ = uploader.create_and_upload_tarball(&entry).await;
    }

    #[tokio::test]
    #[ignore = "requires a running Nexus instance and test tarball"]
    async fn test_upload_tarball() {
        let uploader = NexusUploader::new(test_config()).unwrap();
        let path = PathBuf::from_str("/Users/lih/Downloads/zxing-wasm-1.3.4.tgz").unwrap();
        let res = uploader.client.upload_tarball(&path).await;
        println!("{:?}", res);
    }

    #[tokio::test]
    #[ignore = "requires a running Nexus instance"]
    async fn test_check_package_exists() {
        let uploader = NexusUploader::new(test_config()).unwrap();
        let res = uploader
            .client
            .check_package_exists("babel-sugar-v-on", "1.4.0")
            .await;
        println!("{:?}", res);
        assert!(res.is_ok());
    }

    #[test]
    fn test_scan_smoke() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("lodash");
        fs::create_dir_all(&nm).unwrap();
        fs::write(
            nm.join("package.json"),
            r#"{"name":"lodash","version":"4.17.21"}"#,
        )
        .unwrap();

        let packages = PackageScanner::scan(&dir.path().join("node_modules")).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "lodash");
    }

    #[test]
    fn test_find_node_modules() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir(&nm).unwrap();

        // from project root
        assert!(PackageScanner::find_node_modules(dir.path()).is_ok());
        // from node_modules itself
        assert!(PackageScanner::find_node_modules(&nm).is_ok());
        // missing
        assert!(PackageScanner::find_node_modules(Path::new("/no/such/dir")).is_err());
    }
}
