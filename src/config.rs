use crate::error::{Result, RnuError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: String,
    pub user: String,
    pub password: String,
    pub repository: String,
    pub verbose: bool,
    pub dry_run: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigFile {
    pub default_server: Option<String>,
    pub default_repository: Option<String>,
    pub profiles: Option<std::collections::HashMap<String, ProfileConfig>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub server: String,
    pub repository: Option<String>,
    pub user: Option<String>,
}

impl Config {
    pub fn new(
        server: String,
        user: String,
        password: String,
        repository: String,
        verbose: bool,
        dry_run: bool,
    ) -> Self {
        Config {
            server,
            user,
            password,
            repository,
            verbose,
            dry_run,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.server.is_empty() {
            return Err(RnuError::ConfigError("Server URL is required".to_string()));
        }

        if !self.server.starts_with("http://") && !self.server.starts_with("https://") {
            return Err(RnuError::ConfigError(
                "Server URL must start with http:// or https://".to_string(),
            ));
        }

        if self.user.is_empty() {
            return Err(RnuError::ConfigError("Username is required".to_string()));
        }

        if self.password.is_empty() {
            return Err(RnuError::ConfigError("Password is required".to_string()));
        }

        if self.repository.is_empty() {
            return Err(RnuError::ConfigError(
                "Repository name is required".to_string(),
            ));
        }

        Ok(())
    }
}

impl ConfigFile {
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(ConfigFile {
                default_server: None,
                default_repository: None,
                profiles: None,
            });
        }

        let content = fs::read_to_string(path)?;
        let config: ConfigFile = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save_to_file(&self, path: &PathBuf) -> Result<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn get_profile(&self, profile_name: &str) -> Option<&ProfileConfig> {
        self.profiles
            .as_ref()
            .and_then(|profiles| profiles.get(profile_name))
    }

    pub fn add_profile(&mut self, profile_name: String, profile: ProfileConfig) {
        if self.profiles.is_none() {
            self.profiles = Some(std::collections::HashMap::new());
        }

        if let Some(ref mut profiles) = self.profiles {
            profiles.insert(profile_name, profile);
        }
    }
}

pub fn get_config_dir() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| RnuError::ConfigError("Could not determine config directory".to_string()))?
        .join("rnu");

    Ok(config_dir)
}

pub fn get_default_config_file() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("config.json"))
}

pub fn create_sample_config() -> ConfigFile {
    let mut profiles = std::collections::HashMap::new();

    profiles.insert(
        "local".to_string(),
        ProfileConfig {
            server: "http://localhost:8081".to_string(),
            repository: Some("npm-hosted".to_string()),
            user: Some("admin".to_string()),
        },
    );

    profiles.insert(
        "production".to_string(),
        ProfileConfig {
            server: "https://nexus.company.com".to_string(),
            repository: Some("npm-private".to_string()),
            user: None, // User should be provided via CLI or environment
        },
    );

    ConfigFile {
        default_server: Some("http://localhost:8081".to_string()),
        default_repository: Some("npm-hosted".to_string()),
        profiles: Some(profiles),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_validation() {
        let valid_config = Config::new(
            "http://localhost:8081".to_string(),
            "admin".to_string(),
            "password".to_string(),
            "npm-hosted".to_string(),
            false,
            false,
        );

        assert!(valid_config.validate().is_ok());

        let invalid_config = Config::new(
            "invalid-url".to_string(),
            "admin".to_string(),
            "password".to_string(),
            "npm-hosted".to_string(),
            false,
            false,
        );

        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_config_file_operations() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("test_config.json");

        let config = create_sample_config();
        assert!(config.save_to_file(&config_path).is_ok());
        assert!(config_path.exists());

        let loaded_config = ConfigFile::load_from_file(&config_path).unwrap();
        assert!(loaded_config.profiles.is_some());
        assert!(loaded_config.get_profile("local").is_some());
    }

    #[test]
    fn test_profile_management() {
        let mut config = ConfigFile {
            default_server: None,
            default_repository: None,
            profiles: None,
        };

        let profile = ProfileConfig {
            server: "http://test.com".to_string(),
            repository: Some("test-repo".to_string()),
            user: Some("testuser".to_string()),
        };

        config.add_profile("test".to_string(), profile);
        assert!(config.get_profile("test").is_some());
        assert_eq!(
            config.get_profile("test").unwrap().server,
            "http://test.com"
        );
    }
}
