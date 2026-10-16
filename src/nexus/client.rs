use crate::config::Config;
use crate::error::{Result, RnuError};
use reqwest::Client;
use std::path::PathBuf;
use std::time::Duration;
use tracing::debug;

/// HTTP client for Nexus Repository Manager REST API.
///
/// Wraps `reqwest::Client` with Basic auth and Nexus-specific endpoints.
/// Intentionally cheap to clone (reqwest::Client is an Arc internally).
#[derive(Clone)]
pub struct NexusClient {
    server: String,
    repository: String,
    user: String,
    password: String,
    http: Client,
}

impl NexusClient {
    pub fn new(config: &Config) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("rnu/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            server: config.server.trim_end_matches('/').to_string(),
            repository: config.repository.clone(),
            user: config.user.clone(),
            password: config.password.clone(),
            http,
        })
    }

    /// HEAD-based existence check via Nexus search API.
    pub async fn check_package_exists(&self, name: &str, version: &str) -> Result<bool> {
        let url = format!(
            "{}/service/rest/v1/search?repository={}&name={}&version={}",
            self.server,
            self.repository,
            urlencoding::encode(name),
            urlencoding::encode(version)
        );

        debug!("Checking existence: {}@{}", name, version);

        let response = self
            .http
            .get(&url)
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?;

        if response.status().is_success() {
            let search_result: serde_json::Value = response.json().await?;
            Ok(search_result["items"]
                .as_array()
                .map(|items| !items.is_empty())
                .unwrap_or(false))
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

    /// Upload a single tarball to the Nexus component API.
    pub async fn upload_tarball(&self, tarball_path: &PathBuf) -> Result<()> {
        let url = format!(
            "{}/service/rest/v1/components?repository={}",
            self.server, self.repository,
        );

        debug!("Uploading to: {}", url);
        debug!("tmp file: {}", tarball_path.display());

        let mut file = std::fs::File::open(tarball_path.clone())?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut buf)?;

        let filename = tarball_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid filename")
            })?;

        let part = reqwest::multipart::Part::bytes(buf)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")?;

        let form = reqwest::multipart::Form::new().part("npm.asset", part);

        let response = self
            .http
            .post(&url)
            .basic_auth(&self.user, Some(&self.password))
            .header("accept", "application/json")
            .multipart(form)
            .send()
            .await?;

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
            let text = response.text().await.unwrap_or_default();
            Err(RnuError::UploadError(format!("HTTP {} - {}", status, text)))
        }
    }

    /// Verify connectivity to the Nexus server.
    #[allow(dead_code)]
    pub async fn test_connection(&self) -> Result<()> {
        let url = format!("{}/service/rest/v1/status", self.server);

        let response = self
            .http
            .get(&url)
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?;

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

    // ── Maven-specific methods ───────────────────────────────────

    /// Check whether a Maven artifact (groupId:artifactId:version)
    /// already exists in the repository.
    pub async fn check_maven_exists(
        &self,
        group_id: &str,
        artifact_id: &str,
        version: &str,
    ) -> Result<bool> {
        let url = format!(
            "{}/service/rest/v1/search?repository={}&maven.groupId={}&maven.artifactId={}&maven.version={}",
            self.server,
            self.repository,
            urlencoding::encode(group_id),
            urlencoding::encode(artifact_id),
            urlencoding::encode(version),
        );

        debug!("Checking Maven existence: {}:{}:{}", group_id, artifact_id, version);

        let response = self
            .http
            .get(&url)
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?;

        if response.status().is_success() {
            let result: serde_json::Value = response.json().await?;
            Ok(result["items"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false))
        } else if response.status().as_u16() == 404 {
            Ok(false)
        } else if response.status().as_u16() == 401 {
            Err(RnuError::AuthenticationError("Invalid credentials".to_string()))
        } else {
            Err(RnuError::NetworkError(format!(
                "Failed to check Maven artifact existence: HTTP {}",
                response.status()
            )))
        }
    }

    /// Upload a Maven component with its files.
    pub async fn upload_maven_component(
        &self,
        group_id: &str,
        artifact_id: &str,
        version: &str,
        files: &[(PathBuf, String)],
    ) -> Result<()> {
        let url = format!(
            "{}/service/rest/v1/components?repository={}",
            self.server, self.repository,
        );

        debug!("Uploading Maven: {}:{}:{}", group_id, artifact_id, version);

        let mut form = reqwest::multipart::Form::new()
            .text("maven2.groupId", group_id.to_string())
            .text("maven2.artifactId", artifact_id.to_string())
            .text("maven2.version", version.to_string());

        for (i, (path, ext)) in files.iter().enumerate() {
            let idx = i + 1;
            let data = std::fs::read(path)?;
            let fname = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let part = reqwest::multipart::Part::bytes(data)
                .file_name(fname.to_string())
                .mime_str("application/octet-stream")?;
            form = form
                .part(format!("maven2.asset{}", idx), part)
                .text(format!("maven2.asset{}.extension", idx), ext.clone());
        }

        let response = self
            .http
            .post(&url)
            .basic_auth(&self.user, Some(&self.password))
            .header("accept", "application/json")
            .multipart(form)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else if response.status().as_u16() == 401 {
            Err(RnuError::AuthenticationError("Invalid credentials".to_string()))
        } else if response.status().as_u16() == 403 {
            Err(RnuError::AuthenticationError("Insufficient permissions".to_string()))
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(RnuError::UploadError(format!("HTTP {} - {}", status, text)))
        }
    }
}
