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

/// Merges CLI args, profile, and config defaults into a final Config.
/// Priority: CLI > profile > config defaults > built-in defaults.
pub struct ConfigResolver;

impl ConfigResolver {
    pub fn resolve(
        cli_server: Option<&str>,
        cli_user: Option<&str>,
        cli_password: &str,
        cli_repository: Option<&str>,
        cli_profile: Option<&str>,
        cli_verbose: bool,
        cli_dry_run: bool,
        config_file: &ConfigFile,
    ) -> Result<Config> {
        let server = Self::resolve_server(cli_server, cli_profile, config_file)?;
        let repository = Self::resolve_repository(cli_repository, cli_profile, config_file);
        let user = Self::resolve_user(cli_user, cli_profile, config_file)?;

        Ok(Config::new(
            server,
            user,
            cli_password.to_string(),
            repository,
            cli_verbose,
            cli_dry_run,
        ))
    }

    fn resolve_server(
        cli_server: Option<&str>,
        cli_profile: Option<&str>,
        config_file: &ConfigFile,
    ) -> Result<String> {
        if let Some(server) = cli_server {
            return Ok(server.to_string());
        }
        if let Some(profile_name) = cli_profile {
            return config_file
                .get_profile(profile_name)
                .map(|p| p.server.clone())
                .ok_or_else(|| {
                    RnuError::ConfigError(format!(
                        "Profile '{}' not found in configuration file",
                        profile_name
                    ))
                });
        }
        config_file
            .default_server
            .clone()
            .ok_or_else(|| {
                RnuError::ConfigError(
                    "Server URL must be specified via --server, --profile, or default_server in config file"
                        .to_string(),
                )
            })
    }

    fn resolve_repository(
        cli_repository: Option<&str>,
        cli_profile: Option<&str>,
        config_file: &ConfigFile,
    ) -> String {
        if let Some(repo) = cli_repository {
            return repo.to_string();
        }
        if let Some(profile_name) = cli_profile {
            if let Some(profile) = config_file.get_profile(profile_name) {
                if let Some(repo) = &profile.repository {
                    return repo.clone();
                }
            }
        }
        config_file
            .default_repository
            .clone()
            .unwrap_or_else(|| "npm-hosted".to_string())
    }

    fn resolve_user(
        cli_user: Option<&str>,
        cli_profile: Option<&str>,
        config_file: &ConfigFile,
    ) -> Result<String> {
        if let Some(user) = cli_user {
            return Ok(user.to_string());
        }
        if let Some(profile_name) = cli_profile {
            if let Some(profile) = config_file.get_profile(profile_name) {
                if let Some(user) = &profile.user {
                    return Ok(user.clone());
                }
            }
            return Err(RnuError::ConfigError(
                "Username must be specified via --user or in the profile".to_string(),
            ));
        }
        Err(RnuError::ConfigError(
            "Username must be specified via --user or --profile".to_string(),
        ))
    }
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
