mod config;
mod error;
mod nexus;
mod utils;

use clap::{Parser, Subcommand};
use colored::*;
use config::{create_sample_config, get_default_config_file, Config, ConfigFile};
use error::{Result, RnuError};
use nexus::NexusUploader;
use std::path::PathBuf;
use std::process;
// use utils::{print_error, print_info, print_success, print_warning};

#[derive(Parser)]
#[command(
    name = "rnu",
    version = "0.1.0",
    about = "Rust Nexus Uploader - Upload packages to Nexus repository",
    long_about = "A CLI tool for uploading npm packages to Nexus repository with offline support"
)]
struct Cli {
    /// Nexus server URL (e.g., http://nexus.example.com:8081)
    #[arg(long, short = 's', help = "Nexus server URL")]
    server: Option<String>,

    /// Username for Nexus authentication
    #[arg(long, short = 'u', help = "Username for authentication")]
    user: Option<String>,

    /// Password for Nexus authentication
    #[arg(long, short = 'p', help = "Password for authentication")]
    password: Option<String>,

    /// Repository name in Nexus (defaults to npm-hosted)
    #[arg(long, short = 'r', help = "Repository name")]
    repository: Option<String>,

    /// Use a specific profile from config file
    #[arg(long, help = "Use configuration profile")]
    profile: Option<String>,

    /// Configuration file path
    #[arg(long, help = "Path to configuration file")]
    config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(long, short = 'v', help = "Enable verbose logging")]
    verbose: bool,

    /// Dry run mode (don't actually upload)
    #[arg(long, help = "Dry run - show what would be uploaded without uploading")]
    dry_run: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Upload npm packages from node_modules directory
    Npm {
        /// Path to node_modules directory or project directory containing node_modules
        #[arg(help = "Path to node_modules directory or project root")]
        path: PathBuf,

        /// Skip packages that already exist in repository
        #[arg(long, help = "Skip packages that already exist")]
        skip_existing: bool,

        /// Include dev dependencies
        #[arg(long, help = "Include development dependencies")]
        include_dev: bool,

        /// Only upload specific packages (comma-separated)
        #[arg(long, help = "Only upload specific packages (comma-separated)")]
        only: Option<String>,

        /// Exclude specific packages (comma-separated)
        #[arg(long, help = "Exclude specific packages (comma-separated)")]
        exclude: Option<String>,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Future extension point for other package types
    #[command(hide = true)]
    Maven {
        path: PathBuf,
        #[arg(long)]
        skip_existing: bool,
    },

    #[command(hide = true)]
    Pypi {
        path: PathBuf,
        #[arg(long)]
        skip_existing: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Initialize configuration file with sample data
    Init {
        /// Force overwrite existing config
        #[arg(long)]
        force: bool,
    },
    /// Show current configuration
    Show,
    /// List available profiles
    List,
    /// Add a new profile
    Add {
        /// Profile name
        name: String,
        /// Server URL
        #[arg(long)]
        server: String,
        /// Repository name
        #[arg(long)]
        repository: Option<String>,
        /// Username
        #[arg(long)]
        user: Option<String>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red().bold(), e);
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config { action } => {
            handle_config_command(action)?;
        }
        Commands::Npm {
            ref path,
            skip_existing,
            include_dev,
            ref only,
            ref exclude,
        } => {
            let config = build_config(&cli)?;
            config.validate()?;

            if cli.verbose {
                println!("{}", "Starting RNU - Rust Nexus Uploader".green().bold());
                println!("Server: {}", config.server);
                println!("Repository: {}", config.repository);
                println!("Dry run: {}", config.dry_run);
                println!();
            }

            let uploader = NexusUploader::new(config)?;

            let filter_options = nexus::FilterOptions {
                only_packages: only
                    .as_ref()
                    .map(|s| s.split(',').map(|s| s.trim().to_string()).collect()),
                exclude_packages: exclude
                    .as_ref()
                    .map(|s| s.split(',').map(|s| s.trim().to_string()).collect()),
            };

            uploader.upload_npm_packages(
                path.clone(),
                skip_existing,
                include_dev,
                filter_options,
            )?;
        }
        Commands::Maven {
            path,
            skip_existing: _,
        } => {
            println!("{}", "Maven package upload is not implemented yet".yellow());
            println!("Path: {}", path.display());
            println!("This feature is planned for future releases.");
        }
        Commands::Pypi {
            path,
            skip_existing: _,
        } => {
            println!("{}", "PyPI package upload is not implemented yet".yellow());
            println!("Path: {}", path.display());
            println!("This feature is planned for future releases.");
        }
    }

    Ok(())
}

fn handle_config_command(action: ConfigAction) -> Result<()> {
    let config_path = get_default_config_file()?;

    match action {
        ConfigAction::Init { force } => {
            if config_path.exists() && !force {
                return Err(RnuError::ConfigError(format!(
                    "Configuration file already exists at {}. Use --force to overwrite.",
                    config_path.display()
                )));
            }

            let sample_config = create_sample_config();
            sample_config.save_to_file(&config_path)?;

            println!(
                "{} Configuration file created at: {}",
                "✓".green().bold(),
                config_path.display()
            );
            println!("Edit this file to add your Nexus server details.");
        }
        ConfigAction::Show => {
            if !config_path.exists() {
                println!(
                    "{} No configuration file found at: {}",
                    "!".yellow().bold(),
                    config_path.display()
                );
                println!("Run 'rnu config init' to create a sample configuration.");
                return Ok(());
            }

            let config = ConfigFile::load_from_file(&config_path)?;
            println!("Configuration file: {}", config_path.display());
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        ConfigAction::List => {
            if !config_path.exists() {
                println!("{} No configuration file found.", "!".yellow().bold());
                return Ok(());
            }

            let config = ConfigFile::load_from_file(&config_path)?;
            if let Some(profiles) = config.profiles {
                println!("{}", "Available profiles:".bold());
                for (name, profile) in profiles {
                    println!("  {} - {}", name.cyan(), profile.server);
                    if let Some(repo) = profile.repository {
                        println!("    Repository: {}", repo);
                    }
                    if let Some(user) = profile.user {
                        println!("    User: {}", user);
                    }
                }
            } else {
                println!("No profiles configured.");
            }
        }
        ConfigAction::Add {
            name,
            server,
            repository,
            user,
        } => {
            let mut config = if config_path.exists() {
                ConfigFile::load_from_file(&config_path)?
            } else {
                ConfigFile {
                    default_server: None,
                    default_repository: None,
                    profiles: None,
                }
            };

            let profile = config::ProfileConfig {
                server,
                repository,
                user,
            };

            config.add_profile(name.clone(), profile);
            config.save_to_file(&config_path)?;

            println!(
                "{} Profile '{}' added successfully.",
                "✓".green().bold(),
                name.cyan()
            );
        }
    }

    Ok(())
}

fn build_config(cli: &Cli) -> Result<Config> {
    let config_path = cli.config.clone().unwrap_or_else(|| {
        get_default_config_file().unwrap_or_else(|_| PathBuf::from("rnu-config.json"))
    });

    let config_file = if config_path.exists() {
        ConfigFile::load_from_file(&config_path)?
    } else {
        ConfigFile {
            default_server: None,
            default_repository: None,
            profiles: None,
        }
    };

    // Determine server
    let server = if let Some(server) = &cli.server {
        server.clone()
    } else if let Some(profile_name) = &cli.profile {
        if let Some(profile) = config_file.get_profile(profile_name) {
            profile.server.clone()
        } else {
            return Err(RnuError::ConfigError(format!(
                "Profile '{}' not found in configuration file",
                profile_name
            )));
        }
    } else if let Some(ref default_server) = config_file.default_server {
        default_server.clone()
    } else {
        return Err(RnuError::ConfigError(
            "Server URL must be specified via --server, --profile, or default_server in config file".to_string()
        ));
    };

    // Determine repository
    let repository = if let Some(repo) = &cli.repository {
        repo.clone()
    } else if let Some(profile_name) = &cli.profile {
        if let Some(profile) = config_file.get_profile(profile_name) {
            profile
                .repository
                .clone()
                .unwrap_or_else(|| "npm-hosted".to_string())
        } else {
            "npm-hosted".to_string()
        }
    } else if let Some(ref default_repo) = config_file.default_repository {
        default_repo.clone()
    } else {
        "npm-hosted".to_string()
    };

    // Determine user
    let user = if let Some(user) = &cli.user {
        user.clone()
    } else if let Some(profile_name) = &cli.profile {
        if let Some(profile) = config_file.get_profile(profile_name) {
            if let Some(profile_user) = &profile.user {
                profile_user.clone()
            } else {
                return Err(RnuError::ConfigError(
                    "Username must be specified via --user or in the profile".to_string(),
                ));
            }
        } else {
            return Err(RnuError::ConfigError(
                "Username must be specified via --user".to_string(),
            ));
        }
    } else {
        return Err(RnuError::ConfigError(
            "Username must be specified via --user or --profile".to_string(),
        ));
    };

    // Password must always be provided via CLI for security
    let password = cli
        .password
        .as_ref()
        .ok_or_else(|| {
            RnuError::ConfigError("Password must be specified via --password".to_string())
        })?
        .clone();

    Ok(Config::new(
        server,
        user,
        password,
        repository,
        cli.verbose,
        cli.dry_run,
    ))
}
