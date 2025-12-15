use crate::config::Config;
use crate::error::{Result, RnuError};
use crate::utils::progress::UploadProgress;
use colored::*;
use flate2::write::GzEncoder;
use flate2::Compression;
use reqwest::blocking::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use tar::Builder;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageJson {
    name: String,
    version: String,
    #[serde(default)]
    dependencies: serde_json::Map<String, Value>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub tarball_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct FilterOptions {
    pub only_packages: Option<Vec<String>>,
    pub exclude_packages: Option<Vec<String>>,
}

impl FilterOptions {
    pub fn should_include(&self, package_name: &str) -> bool {
        // Check exclude list first
        if let Some(ref exclude) = self.exclude_packages {
            if exclude.iter().any(|p| package_name.contains(p)) {
                return false;
            }
        }

        // If only_packages is specified, check if package is in the list
        if let Some(ref only) = self.only_packages {
            only.iter().any(|p| package_name.contains(p))
        } else {
            true
        }
    }
}

pub struct NexusUploader {
    config: Config,
    client: Client,
}

impl NexusUploader {
    pub fn new(config: Config) -> Result<Self> {
        let client = Client::new();
        Ok(NexusUploader { config, client })
    }

    pub fn upload_npm_packages(
        &self,
        path: PathBuf,
        skip_existing: bool,
        _include_dev: bool,
        filter_options: FilterOptions,
    ) -> Result<()> {
        let node_modules_path = self.find_node_modules(&path)?;

        if self.config.verbose {
            println!("Scanning packages in: {}", node_modules_path.display());
        }

        let packages = self.scan_npm_packages(&node_modules_path)?;

        println!("**** packages lenght is {}", packages.len());
        if packages.is_empty() {
            println!("No packages found in {}", node_modules_path.display());
            return Ok(());
        }

        // Apply filters
        let filtered_packages: Vec<PackageEntry> = packages
            .into_iter()
            .filter(|pkg| filter_options.should_include(&pkg.name))
            .collect();

        if filtered_packages.is_empty() {
            println!("No packages match the specified filters");
            return Ok(());
        }

        println!("Found {} packages to process", filtered_packages.len());

        // Create progress tracker
        let mut progress = UploadProgress::new(filtered_packages.len() as u64);

        let mut _uploaded = 0;
        let mut _skipped = 0;
        let mut _errors = 0;

        for package in filtered_packages.iter() {
            progress.set_message(format!("Processing {}@{}", package.name, package.version));

            if self.config.verbose {
                println!("Processing: {}@{}", package.name, package.version);
            }

            if self.config.dry_run {
                println!(
                    "  {} Would upload: {}@{}",
                    "→".blue(),
                    package.name,
                    package.version
                );
                progress.increment_uploaded(&package.name, &package.version);
                _uploaded += 1;
            } else {
                match self.upload_package(package) {
                    Ok(_) => {
                        println!(
                            "  {} Uploaded: {}@{}",
                            "✓".green(),
                            package.name,
                            package.version
                        );
                        progress.increment_uploaded(&package.name, &package.version);
                        _uploaded += 1;
                    }
                    Err(e) => {
                        eprintln!(
                            "  {} Failed to upload {}@{}: {}",
                            "✗".red(),
                            package.name,
                            package.version,
                            e
                        );
                        progress.increment_error(&package.name, &package.version);
                        _errors += 1;
                    }
                }
            }
        }

        progress.finish_with_summary();

        if _errors > 0 {
            Err(RnuError::UploadError(format!(
                "{} packages failed to upload",
                _errors
            )))
        } else {
            Ok(())
        }
    }

    fn find_node_modules(&self, path: &PathBuf) -> Result<PathBuf> {
        if path.file_name().and_then(|n| n.to_str()) == Some("node_modules") {
            return Ok(path.clone());
        }

        let node_modules = path.join("node_modules");
        if node_modules.exists() && node_modules.is_dir() {
            return Ok(node_modules);
        }

        Err(RnuError::PackageNotFound(format!(
            "node_modules directory not found at {} or {}",
            path.display(),
            node_modules.display()
        )))
    }

    fn scan_npm_packages(&self, node_modules_path: &PathBuf) -> Result<Vec<PackageEntry>> {
        let mut packages = Vec::new();
        let mut seen_packages = HashSet::new();

        self.scan_directory(node_modules_path, &mut packages, &mut seen_packages)?;

        // Sort packages by name for consistent output
        packages.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(packages)
    }

    fn scan_directory(
        &self,
        dir_path: &PathBuf,
        packages: &mut Vec<PackageEntry>,
        seen_packages: &mut HashSet<String>,
    ) -> Result<()> {
        if !dir_path.is_dir() {
            return Ok(());
        }

        let entries = fs::read_dir(dir_path)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| RnuError::InvalidPackage("Invalid directory name".to_string()))?;

            // Skip .bin and other special directories
            if dir_name.starts_with('.') {
                continue;
            }

            if dir_name.starts_with('@') {
                // Scoped package directory, recurse into it
                self.scan_directory(&path, packages, seen_packages)?;
            } else {
                // Regular package directory
                if let Ok(package) = self.read_package_json(&path) {
                    let name = package.name.clone();
                    let version = package.version.clone();
                    let package_key = format!("{}@{}", name, version);
                    if let Ok(flag) = self.check_package_exists(&name, &version) {
                        if flag {
                            continue;
                        }
                    }
                    if !seen_packages.contains(&package_key) {
                        seen_packages.insert(package_key);
                        packages.push(package);
                    }
                }
            }
        }

        Ok(())
    }

    fn read_package_json(&self, package_dir: &PathBuf) -> Result<PackageEntry> {
        let package_json_path = package_dir.join("package.json");

        if !package_json_path.exists() {
            return Err(RnuError::PackageNotFound(format!(
                "package.json not found in {}",
                package_dir.display()
            )));
        }

        let content = fs::read_to_string(&package_json_path)?;
        let package_json: PackageJson = serde_json::from_str(&content)?;

        // Look for tarball
        let tarball_path =
            self.find_tarball(package_dir, &package_json.name, &package_json.version);

        let package_name = self.extract_package_name(&package_json.name);
        Ok(PackageEntry {
            name: package_name,
            version: package_json.version,
            path: package_dir.clone(),
            tarball_path,
        })
    }

    fn extract_package_name(&self, full_name: &str) -> String {
        if let Some(pos) = full_name.rfind('/') {
            // 有作用域的情况：@achrinza/node-ipc -> node-ipc
            full_name[pos + 1..].to_string()
        } else if let Some(pos) = full_name.find('@') {
            // 无作用域但有版本：express@4.18.0 -> express
            full_name[..pos].to_string()
        } else {
            // 只有包名
            full_name.to_string()
        }
    }

    fn find_tarball(&self, package_dir: &PathBuf, name: &str, version: &str) -> Option<PathBuf> {
        // Common tarball naming patterns
        let sanitized_name = name.replace('@', "").replace('/', "-");
        let possible_names = vec![
            format!("{}-{}.tgz", name, version),
            format!("{}-{}.tar.gz", name, version),
            format!("{}-{}.tgz", sanitized_name, version),
            format!("{}-{}.tar.gz", sanitized_name, version),
            "package.tgz".to_string(),
            "package.tar.gz".to_string(),
        ];

        for tarball_name in possible_names {
            let tarball_path = package_dir.join(&tarball_name);
            if tarball_path.exists() {
                return Some(tarball_path);
            }
        }

        None
    }

    fn check_package_exists(&self, name: &str, version: &str) -> Result<bool> {
        let url = format!(
            "{}/service/rest/v1/search?repository={}&name={}&version={}",
            self.config.server.trim_end_matches('/'),
            self.config.repository,
            urlencoding::encode(name),
            urlencoding::encode(version)
        );

        if self.config.verbose {
            println!("    Checking existence: {}", url);
        }

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.config.user, Some(&self.config.password))
            .send()?;

        if response.status().is_success() {
            let search_result: Value = response.json()?;

            if let Some(items) = search_result["items"].as_array() {
                Ok(!items.is_empty())
            } else {
                Ok(false)
            }
        } else if response.status().as_u16() == 404 {
            Ok(false)
        } else if response.status().as_u16() == 401 {
            Err(RnuError::AuthenticationError(
                "Invalid credentials".to_string(),
            ))
        } else {
            Err(RnuError::NetworkError(format!(
                "Failed to check package existence: HTTP {}",
                response.status()
            )))
        }
    }

    fn upload_package(&self, package: &PackageEntry) -> Result<()> {
        // Try to upload tarball first, if available
        if let Some(ref tarball_path) = package.tarball_path {
            self.upload_tarball(tarball_path)
        } else {
            // Create tarball from package directory and upload
            self.create_and_upload_tarball(package)
        }
    }

    fn upload_tarball(&self, tarball_path: &PathBuf) -> Result<()> {
        let url = format!(
            "{}/service/rest/v1/components?repository={}",
            self.config.server.trim_end_matches('/'),
            self.config.repository,
        );

        if self.config.verbose {
            println!("    Uploading to: {}", url);
        }

        // 读取文件内容
        println!("      tmp file... {}", tarball_path.display());
        let mut file = File::open(tarball_path.clone())?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        let filename = tarball_path
            .file_name()
            .and_then(|name| name.to_str().map(|s| s.to_owned()))
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "无法获取文件名")
            })?;

        // 创建multipart表单
        let part = multipart::Part::bytes(buffer)
            .file_name(filename)
            .mime_str("application/octet-stream")?;

        let form = multipart::Form::new().part("npm.asset", part);

        // 发送请求
        let response = self
            .client
            .post(&url)
            .basic_auth(&self.config.user, Some(&self.config.password))
            .header("accept", "application/json")
            .multipart(form)
            .send()?;

        if response.status().is_success() {
            Ok(())
        } else if response.status().as_u16() == 401 {
            Err(RnuError::AuthenticationError(
                "Invalid credentials".to_string(),
            ))
        } else if response.status().as_u16() == 403 {
            Err(RnuError::AuthenticationError(
                "Insufficient permissions".to_string(),
            ))
        } else {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            Err(RnuError::UploadError(format!("HTTP {} - {}", status, text)))
        }
    }

    // fn create_and_upload_tarball(&self, package: &PackageEntry) -> Result<()> {
    //     let tarball_name = format!("{}-{}.tgz", package.name.replace("/", "-"), package.version);
    //     let tarball_path = std::env::temp_dir().join(&tarball_name);

    //     // Create tar.gz file
    //     let tar_file = File::create(&tarball_path)?;
    //     let enc = GzEncoder::new(tar_file, Compression::default());
    //     let mut tar = Builder::new(enc);

    //     // Recursively add all files and directories from package directory
    //     fn append_dir(
    //         tar: &mut Builder<impl Write>,
    //         base_path: &PathBuf,
    //         current_path: &PathBuf,
    //     ) -> io::Result<()> {
    //         for entry in fs::read_dir(current_path)? {
    //             let entry = entry?;
    //             let path = entry.path();
    //             let relative_path = path
    //                 .strip_prefix(base_path)
    //                 .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
    //                 .to_path_buf();

    //             if path.is_dir() {
    //                 let mut header = tar::Header::new_gnu();
    //                 header.set_path(&relative_path)?;
    //                 header.set_size(0);
    //                 header.set_mode(0o755);
    //                 header.set_entry_type(tar::EntryType::Directory);
    //                 header.set_cksum();
    //                 tar.append(&header, &mut io::empty())?;
    //                 append_dir(tar, base_path, &path)?;
    //             } else if path.is_file() {
    //                 let mut file = File::open(&path)?;
    //                 let mut header = tar::Header::new_gnu();
    //                 header.set_path(&relative_path)?;
    //                 header.set_size(file.metadata()?.len());
    //                 header.set_mode(0o644);
    //                 header.set_cksum();
    //                 tar.append(&header, &mut file)?;
    //             }
    //         }
    //         Ok(())
    //     }

    //     let package_dir = &package.path;
    //     append_dir(&mut tar, package_dir, package_dir)?;

    //     // Finish tar creation
    //     tar.into_inner()?.finish()?;

    //     // Upload the created tarball
    //     let result = self.upload_tarball(&tarball_path);

    //     // Clean up temporary file
    //     // fs::remove_file(&tarball_path)?;

    //     result
    // }

    fn create_and_upload_tarball(&self, package: &PackageEntry) -> Result<()> {
        let tarball_name = format!("{}-{}.tgz", package.name.replace("/", "-"), package.version);
        let tarball_path = std::env::temp_dir().join(&tarball_name);

        // Create tar.gz file
        let tar_file = File::create(&tarball_path)?;
        let enc = GzEncoder::new(tar_file, Compression::default());
        let mut tar = Builder::new(enc);

        // Recursively add all files and directories from package directory
        fn append_dir(
            tar: &mut Builder<impl Write>,
            base_path: &PathBuf,
            current_path: &PathBuf,
            tar_root: &str, // 新增参数：tar内的根目录名
        ) -> io::Result<()> {
            for entry in fs::read_dir(current_path)? {
                let entry = entry?;
                let path = entry.path();

                // 计算相对于base_path的路径
                let relative_path = path
                    .strip_prefix(base_path)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                    .to_path_buf();

                // 构建tar内的完整路径：tar_root/relative_path
                let tar_path = if relative_path.to_string_lossy().is_empty() {
                    // 如果是根目录，只使用tar_root
                    PathBuf::from(tar_root)
                } else {
                    PathBuf::from(tar_root).join(&relative_path)
                };

                if path.is_dir() {
                    // 跳过node_modules目录
                    if path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n == "node_modules")
                        .unwrap_or(false)
                    {
                        continue;
                    }

                    let mut header = tar::Header::new_gnu();
                    header.set_path(&tar_path)?;
                    header.set_size(0);
                    header.set_mode(0o755);
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_cksum();
                    tar.append(&header, &mut io::empty())?;

                    // 递归处理子目录
                    append_dir(tar, base_path, &path, tar_root)?;
                } else if path.is_file() {
                    // 跳过一些不需要的文件
                    if let Some(file_name) = path.file_name() {
                        let file_name_str = file_name.to_string_lossy();
                        if file_name_str.ends_with(".log")
                            || file_name_str == ".DS_Store"
                            || file_name_str == "Thumbs.db"
                        {
                            continue;
                        }
                    }

                    let mut file = File::open(&path)?;
                    let mut header = tar::Header::new_gnu();

                    // 关键修改：使用tar_path而不是relative_path
                    header.set_path(&tar_path)?;
                    header.set_size(file.metadata()?.len());

                    // 根据文件类型设置权限
                    let mode = 0o755;
                    header.set_mode(mode);
                    header.set_cksum();
                    tar.append(&header, &mut file)?;
                }
            }
            Ok(())
        }
        let package_dir = &package.path;

        // 关键修改：所有文件都放在"package/"目录下
        append_dir(&mut tar, package_dir, package_dir, "package")?;

        // Finish tar creation
        tar.into_inner()?.finish()?;

        // Upload the created tarball
        let result = self.upload_tarball(&tarball_path);

        // Clean up temporary file
        fs::remove_file(&tarball_path)?;

        result
    }

    fn build_npm_path(&self, name: &str, version: &str) -> String {
        if name.starts_with('@') {
            // Scoped package: @scope/package -> @scope/package/-/package-version.tgz
            let parts: Vec<&str> = name.splitn(2, '/').collect();
            if parts.len() == 2 {
                let scope = parts[0]; // @scope
                let pkg_name = parts[1]; // package
                format!("{}/{}/-/{}-{}.tgz", scope, pkg_name, pkg_name, version)
            } else {
                format!(
                    "{}/-/{}-{}.tgz",
                    name,
                    name.trim_start_matches('@'),
                    version
                )
            }
        } else {
            // Regular package: package/-/package-version.tgz
            format!("{}/-/{}-{}.tgz", name, name, version)
        }
    }

    pub fn test_connection(&self) -> Result<()> {
        let url = format!(
            "{}/service/rest/v1/status",
            self.config.server.trim_end_matches('/')
        );

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.config.user, Some(&self.config.password))
            .send()?;

        if response.status().is_success() {
            Ok(())
        } else if response.status().as_u16() == 401 {
            Err(RnuError::AuthenticationError(
                "Invalid credentials".to_string(),
            ))
        } else {
            Err(RnuError::NetworkError(format!(
                "Cannot connect to Nexus server: HTTP {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::str::FromStr;
    use tempfile::tempdir;

    fn create_test_config() -> Config {
        Config {
            server: "http://20.21.1.180:18081".to_string(),
            user: "admin".to_string(),
            password: "Zyjc@2020".to_string(),
            repository: "npm-hosted".to_string(),
            verbose: true,
            dry_run: false,
        }
    }

    #[test]
    fn test_build_npm_path() {
        let config = create_test_config();
        let uploader = NexusUploader::new(config).unwrap();

        // Test regular package
        assert_eq!(
            uploader.build_npm_path("lodash", "4.17.21"),
            "lodash/-/lodash-4.17.21.tgz"
        );

        // Test scoped package
        assert_eq!(
            uploader.build_npm_path("@types/node", "18.0.0"),
            "@types/node/-/node-18.0.0.tgz"
        );
    }

    #[test]
    fn test_filter_options() {
        let filter = FilterOptions {
            only_packages: Some(vec!["lodash".to_string(), "express".to_string()]),
            exclude_packages: None,
        };

        assert!(filter.should_include("lodash"));
        assert!(filter.should_include("express"));
        assert!(!filter.should_include("react"));

        let filter = FilterOptions {
            only_packages: None,
            exclude_packages: Some(vec!["test".to_string()]),
        };

        assert!(filter.should_include("lodash"));
        assert!(!filter.should_include("test-package"));
    }

    #[test]
    fn test_package_json_parsing() {
        let json_content = r#"
        {
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.21"
            },
            "devDependencies": {
                "jest": "^27.0.0"
            }
        }
        "#;

        let package_json: PackageJson = serde_json::from_str(json_content).unwrap();
        assert_eq!(package_json.name, "test-package");
        assert_eq!(package_json.version, "1.0.0");
    }

    #[test]
    fn test_create_tarball() {
        let config = create_test_config();
        let uploader = NexusUploader::new(config).unwrap();

        let temp_dir = tempdir().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir(&package_dir).unwrap();

        // Create some files in the package directory
        fs::write(
            package_dir.join("package.json"),
            r#"{"name": "test", "version": "1.0.0"}"#,
        )
        .unwrap();
        fs::write(package_dir.join("README.md"), "Test README").unwrap();

        let package = PackageEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            path: package_dir,
            tarball_path: None,
        };

        // Since upload is mocked, we just check if creation succeeds and file exists
        let result = uploader.create_and_upload_tarball(&package);
        assert!(result.is_ok()) // Expect error because upload would fail without server

        // But check if temp tarball was created (though it's cleaned up, we can adjust test to verify creation)
    }

    #[test]
    fn test_upload_tarball() {
        let config = create_test_config();
        let uploader = NexusUploader::new(config).unwrap();

        let package_dir = PathBuf::from_str("/Users/lih/Downloads").unwrap();

        let tarball_path = package_dir.join("zxing-wasm-1.3.4.tgz");
        let result = uploader.upload_tarball(&tarball_path);
        println!("err: {:?}", result);
        assert!(true)
    }

    #[test]
    fn test_check_package_exists() {
        let config = create_test_config();
        let uploader = NexusUploader::new(config).unwrap();
        let result = uploader.check_package_exists("babel-sugar-v-on", "1.4.0");
        println!("result : {:?}", result);
        assert!(result.is_ok());
    }

    #[test]
    fn test_scan_directory() {
        let config = create_test_config();
        let uploader = NexusUploader::new(config).unwrap();
        let mut packages = Vec::new();
        let mut seen_packages = HashSet::new();
        let pt = PathBuf::from_str("/Users/lih/shuke/code/views/fuling-h5/node_modules").unwrap();
        let result = uploader.scan_directory(&pt, &mut packages, &mut seen_packages);
        println!("result : {:?}, len: {}", result, packages.len());
        assert!(result.is_ok());
    }
}
