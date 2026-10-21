use crate::error::{Result, RnuError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Default file name, looked up in the current working directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Package ecosystem. Repository resolution and defaults are per-kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Npm,
    Maven,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Npm => "npm",
            Kind::Maven => "maven",
        }
    }

    /// Only npm has a meaningful built-in default, kept for backward
    /// compatibility. Maven repositories are always named by the user, so
    /// guessing one would silently upload to the wrong place.
    fn legacy_default_repository(self) -> Option<&'static str> {
        match self {
            Kind::Npm => Some("npm-hosted"),
            Kind::Maven => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: String,
    pub user: String,
    pub password: String,
    pub repository: String,
    pub verbose: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_password: Option<String>,
    /// Legacy single repository, applies to npm only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_repository: Option<String>,
    /// Repository per ecosystem, e.g. `{"npm": "npm-hosted", "maven": "maven-releases"}`.
    ///
    /// `BTreeMap` rather than `HashMap` so a rewritten config file keeps a
    /// stable key order — it now lives next to the project and may well be
    /// committed, where reshuffled keys show up as diff noise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<BTreeMap<String, ProfileConfig>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub server: String,
    /// Legacy single repository, applies to npm only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
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
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(ConfigFile::default());
        }

        let content = fs::read_to_string(path)?;
        match Format::of(path) {
            Format::Toml => toml::from_str(&content).map_err(|e| {
                RnuError::ConfigError(format!("Failed to parse {} as TOML: {}", path.display(), e))
            }),
            Format::Json => serde_json::from_str(&content).map_err(|e| {
                RnuError::ConfigError(format!("Failed to parse {} as JSON: {}", path.display(), e))
            }),
        }
    }

    /// Render in the format implied by the file extension: `.json` stays JSON
    /// (so `--config old.json` keeps working), everything else is TOML.
    pub fn render(&self, path: &Path) -> Result<String> {
        match Format::of(path) {
            Format::Toml => toml::to_string_pretty(self).map_err(|e| {
                RnuError::ConfigError(format!("Failed to serialise config as TOML: {}", e))
            }),
            Format::Json => Ok(serde_json::to_string_pretty(self)?),
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::write(path, self.render(path)?)?;
        restrict_permissions(path)?;
        Ok(())
    }

    pub fn get_profile(&self, profile_name: &str) -> Option<&ProfileConfig> {
        self.profiles
            .as_ref()
            .and_then(|profiles| profiles.get(profile_name))
    }

    pub fn add_profile(&mut self, profile_name: String, profile: ProfileConfig) {
        self.profiles
            .get_or_insert_with(BTreeMap::new)
            .insert(profile_name, profile);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Toml,
    Json,
}

impl Format {
    fn of(path: &Path) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("json") => Format::Json,
            _ => Format::Toml,
        }
    }
}

/// The config file may hold a password, so keep it private to the owner.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// The config file lives in the **current directory** so that every project can
/// carry its own Nexus settings instead of sharing one global file.
pub fn get_default_config_file() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join(CONFIG_FILE_NAME))
}

/// Where the pre-0.3 releases kept a global `config.json`. It is no longer read;
/// this only exists so the CLI can point users at it once, instead of silently
/// pretending their credentials disappeared.
pub fn legacy_config_file() -> Option<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("rnu").join("config.json"))
        .filter(|path| path.exists())
}

/// Everything the CLI (or the environment) supplied for this invocation.
pub struct ResolveInput<'a> {
    pub server: Option<&'a str>,
    pub user: Option<&'a str>,
    pub password: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub profile: Option<&'a str>,
    pub verbose: bool,
    pub dry_run: bool,
}

/// Merges CLI args, environment (already folded into the CLI struct by clap),
/// profile, and config defaults into a final Config.
///
/// Priority: CLI/env > profile > config defaults > (npm only) built-in default.
pub struct ConfigResolver;

impl ConfigResolver {
    pub fn resolve(kind: Kind, input: &ResolveInput, file: &ConfigFile) -> Result<Config> {
        let profile = match input.profile {
            Some(name) => Some(file.get_profile(name).ok_or_else(|| {
                RnuError::ConfigError(format!(
                    "Profile '{}' not found in configuration file",
                    name
                ))
            })?),
            None => None,
        };

        let server = input
            .server
            .map(str::to_string)
            .or_else(|| profile.map(|p| p.server.clone()))
            .or_else(|| file.default_server.clone())
            .ok_or_else(|| {
                RnuError::ConfigError(
                    "Nexus server is not configured. Use -s/--server, set RNU_SERVER, or run `nexus-upload config init`."
                        .to_string(),
                )
            })?;

        let user = input
            .user
            .map(str::to_string)
            .or_else(|| profile.and_then(|p| p.user.clone()))
            .or_else(|| file.default_user.clone())
            .ok_or_else(|| {
                RnuError::ConfigError(
                    "Username is not configured. Use -u/--user, set RNU_USER, or run `nexus-upload config init`."
                        .to_string(),
                )
            })?;

        let password = input
            .password
            .map(str::to_string)
            .or_else(|| profile.and_then(|p| p.password.clone()))
            .or_else(|| file.default_password.clone())
            .ok_or_else(|| {
                RnuError::ConfigError(
                    "Password is not configured. Use -p/--password, set RNU_PASSWORD, or run `nexus-upload config init`."
                        .to_string(),
                )
            })?;

        let repository = Self::resolve_repository(kind, input, profile, file)?;

        Ok(Config::new(
            server,
            user,
            password,
            repository,
            input.verbose,
            input.dry_run,
        ))
    }

    fn resolve_repository(
        kind: Kind,
        input: &ResolveInput,
        profile: Option<&ProfileConfig>,
        file: &ConfigFile,
    ) -> Result<String> {
        if let Some(repo) = input.repository {
            return Ok(repo.to_string());
        }

        let from_profile = profile
            .and_then(|p| p.repositories.as_ref())
            .and_then(|repos| repos.get(kind.as_str()))
            .or_else(|| {
                // Legacy single-repository field only ever meant npm.
                if kind == Kind::Npm {
                    profile.and_then(|p| p.repository.as_ref())
                } else {
                    None
                }
            });
        if let Some(repo) = from_profile {
            return Ok(repo.clone());
        }

        let from_file = file
            .repositories
            .as_ref()
            .and_then(|repos| repos.get(kind.as_str()))
            .or_else(|| {
                if kind == Kind::Npm {
                    file.default_repository.as_ref()
                } else {
                    None
                }
            });
        if let Some(repo) = from_file {
            return Ok(repo.clone());
        }

        if let Some(fallback) = kind.legacy_default_repository() {
            return Ok(fallback.to_string());
        }

        Err(RnuError::ConfigError(format!(
            "Repository for {} is not configured. Use -r/--repository, set RNU_REPO, \
             or add `{} = \"<repo>\"` under [repositories] in {} \
             (`nexus-upload config init`).",
            kind.as_str(),
            kind.as_str(),
            CONFIG_FILE_NAME
        )))
    }
}

pub fn create_sample_config() -> ConfigFile {
    let mut repositories = BTreeMap::new();
    repositories.insert("npm".to_string(), "npm-hosted".to_string());
    repositories.insert("maven".to_string(), "maven-releases".to_string());

    let mut profiles = BTreeMap::new();
    profiles.insert(
        "local".to_string(),
        ProfileConfig {
            server: "http://localhost:8081".to_string(),
            repository: None,
            repositories: Some(repositories.clone()),
            user: Some("admin".to_string()),
            password: None,
        },
    );
    profiles.insert(
        "production".to_string(),
        ProfileConfig {
            server: "https://nexus.company.com".to_string(),
            repository: None,
            repositories: Some(repositories.clone()),
            user: Some("deployer".to_string()),
            password: None,
        },
    );

    ConfigFile {
        default_server: Some("http://localhost:8081".to_string()),
        default_user: Some("admin".to_string()),
        default_password: None,
        default_repository: None,
        repositories: Some(repositories),
        profiles: Some(profiles),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn file_with(repositories: &[(&str, &str)], legacy: Option<&str>) -> ConfigFile {
        let mut map = BTreeMap::new();
        for (kind, repo) in repositories {
            map.insert(kind.to_string(), repo.to_string());
        }
        ConfigFile {
            default_server: Some("http://nexus:8081".to_string()),
            default_user: Some("admin".to_string()),
            default_password: Some("secret".to_string()),
            default_repository: legacy.map(str::to_string),
            repositories: Some(map),
            profiles: None,
        }
    }

    fn input() -> ResolveInput<'static> {
        ResolveInput {
            server: None,
            user: None,
            password: None,
            repository: None,
            profile: None,
            verbose: false,
            dry_run: false,
        }
    }

    #[test]
    fn test_config_validation() {
        let valid = Config::new(
            "http://localhost:8081".to_string(),
            "admin".to_string(),
            "password".to_string(),
            "npm-hosted".to_string(),
            false,
            false,
        );
        assert!(valid.validate().is_ok());

        let invalid = Config::new(
            "invalid-url".to_string(),
            "admin".to_string(),
            "password".to_string(),
            "npm-hosted".to_string(),
            false,
            false,
        );
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_config_file_operations() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let config = create_sample_config();
        config.save_to_file(&config_path).unwrap();
        assert!(config_path.exists());

        let loaded = ConfigFile::load_from_file(&config_path).unwrap();
        assert!(loaded.get_profile("local").is_some());
    }

    #[test]
    fn toml_round_trip_keeps_every_field() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let config = create_sample_config();
        config.save_to_file(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[repositories]"), "not TOML:\n{text}");
        assert!(text.contains("[profiles.local]"), "not TOML:\n{text}");

        let loaded = ConfigFile::load_from_file(&path).unwrap();
        assert_eq!(loaded.default_server, config.default_server);
        assert_eq!(loaded.repositories, config.repositories);
        assert_eq!(
            loaded.get_profile("production").unwrap().user.as_deref(),
            Some("deployer")
        );
    }

    #[test]
    fn json_config_is_still_readable() {
        // Users who point --config at an old JSON file must keep working.
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        fs::write(
            &path,
            r#"{"default_server":"http://nexus:8081","default_user":"admin",
                "repositories":{"maven":"maven-releases"}}"#,
        )
        .unwrap();

        let loaded = ConfigFile::load_from_file(&path).unwrap();
        assert_eq!(loaded.default_server.as_deref(), Some("http://nexus:8081"));
        assert_eq!(
            loaded
                .repositories
                .as_ref()
                .and_then(|r| r.get("maven"))
                .map(String::as_str),
            Some("maven-releases")
        );

        // And it is written back as JSON, not silently converted to TOML.
        loaded.save_to_file(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.trim_start().starts_with('{'), "got: {text}");
    }

    #[test]
    fn default_config_file_lives_in_the_current_directory() {
        let path = get_default_config_file().unwrap();
        assert_eq!(path.file_name().unwrap(), CONFIG_FILE_NAME);
        assert_eq!(
            path.parent().unwrap(),
            std::env::current_dir().unwrap().as_path()
        );
    }

    #[test]
    fn broken_config_names_the_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "this is not = toml [").unwrap();

        let err = ConfigFile::load_from_file(&path).unwrap_err().to_string();
        assert!(err.contains("config.toml"), "unexpected error: {err}");
        assert!(err.contains("TOML"), "unexpected error: {err}");
    }

    #[test]
    fn test_profile_management() {
        let mut config = ConfigFile::default();
        config.add_profile(
            "test".to_string(),
            ProfileConfig {
                server: "http://test.com".to_string(),
                ..Default::default()
            },
        );
        assert_eq!(
            config.get_profile("test").unwrap().server,
            "http://test.com"
        );
    }

    #[test]
    fn repository_is_resolved_per_kind() {
        let file = file_with(&[("npm", "npm-hosted"), ("maven", "maven-releases")], None);

        let npm = ConfigResolver::resolve(Kind::Npm, &input(), &file).unwrap();
        assert_eq!(npm.repository, "npm-hosted");

        let maven = ConfigResolver::resolve(Kind::Maven, &input(), &file).unwrap();
        assert_eq!(maven.repository, "maven-releases");
    }

    #[test]
    fn maven_never_falls_back_to_the_npm_repository() {
        // Legacy config: a single default_repository that only ever meant npm.
        let file = file_with(&[], Some("npm-hosted"));
        let err = ConfigResolver::resolve(Kind::Maven, &input(), &file).unwrap_err();
        assert!(
            err.to_string().contains("Repository for maven"),
            "unexpected error: {err}"
        );

        // npm keeps the legacy fallback.
        let npm = ConfigResolver::resolve(Kind::Npm, &input(), &file).unwrap();
        assert_eq!(npm.repository, "npm-hosted");
    }

    #[test]
    fn cli_repository_beats_everything() {
        let file = file_with(&[("maven", "from-file")], None);
        let cli = ResolveInput {
            repository: Some("from-cli"),
            ..input()
        };
        let resolved = ConfigResolver::resolve(Kind::Maven, &cli, &file).unwrap();
        assert_eq!(resolved.repository, "from-cli");
    }

    #[test]
    fn profile_repositories_beat_file_level_ones() {
        let mut file = file_with(&[("maven", "file-level")], None);
        let mut repos = BTreeMap::new();
        repos.insert("maven".to_string(), "profile-level".to_string());
        file.add_profile(
            "corp".to_string(),
            ProfileConfig {
                server: "http://profile:8081".to_string(),
                repositories: Some(repos),
                ..Default::default()
            },
        );

        let with_profile = ResolveInput {
            profile: Some("corp"),
            ..input()
        };
        let resolved = ConfigResolver::resolve(Kind::Maven, &with_profile, &file).unwrap();
        assert_eq!(resolved.repository, "profile-level");
        assert_eq!(resolved.server, "http://profile:8081");
    }

    #[test]
    fn missing_credentials_name_the_alternatives() {
        let file = ConfigFile::default();
        let err = ConfigResolver::resolve(Kind::Npm, &input(), &file).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("RNU_SERVER"), "unexpected error: {text}");
    }
}
