mod cli;
mod config;
mod error;
mod nexus;
mod utils;

use clap::Parser;
use cli::{Cli, Commands, ConfigAction};
use colored::*;
use config::{create_sample_config, get_default_config_file, ConfigFile, ConfigResolver};
use error::{Result, RnuError};
use nexus::{FilterOptions, NexusUploader};
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    if let Err(e) = run(cli).await {
        error!("{}", e);
        eprintln!("{} {}", "Error:".red().bold(), e);
        process::exit(1);
    }
}

fn init_tracing(verbose: bool) {
    let default_level = if verbose { "info" } else { "warn" };
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .without_time()
        .with_writer(std::io::stdout)
        .init();
}

async fn run(cli: Cli) -> Result<()> {
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
            concurrency,
        } => {
            let config = build_config(&cli)?;
            config.validate()?;

            info!("Starting RNU - Rust Nexus Uploader");
            info!(server = %config.server, repository = %config.repository, dry_run = config.dry_run);

            let uploader = Arc::new(NexusUploader::new(config)?);
            let filter_options = FilterOptions::from_cli(only.as_deref(), exclude.as_deref());

            uploader
                .run(
                    path.clone(),
                    skip_existing,
                    include_dev,
                    filter_options,
                    concurrency,
                )
                .await?;
        }
        Commands::Maven {
            ref path,
            skip_existing,
            ref only,
            ref exclude,
            concurrency,
        } => {
            let config = build_config(&cli)?;
            config.validate()?;

            info!("Starting RNU - Maven upload");
            info!(server = %config.server, repository = %config.repository, dry_run = config.dry_run);

            let uploader = Arc::new(NexusUploader::new(config)?);
            let filter_options = FilterOptions::from_cli(only.as_deref(), exclude.as_deref());

            uploader
                .run_maven(path.clone(), skip_existing, filter_options, concurrency)
                .await?;
        }
        Commands::Pypi {
            path,
            skip_existing: _,
        } => {
            warn!("PyPI package upload is not implemented yet");
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

            info!(path = %config_path.display(), "Configuration file created");
            println!(
                "{} Configuration file created at: {}",
                "✓".green().bold(),
                config_path.display()
            );
            println!("Edit this file to add your Nexus server details.");
        }
        ConfigAction::Show => {
            if !config_path.exists() {
                warn!(path = %config_path.display(), "No configuration file found");
                println!(
                    "{} No configuration file found at: {}",
                    "!".yellow().bold(),
                    config_path.display()
                );
                println!("Run 'nexus-upload config init' to create a sample configuration.");
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

            info!(%name, "Profile added");
            println!(
                "{} Profile '{}' added successfully.",
                "✓".green().bold(),
                name.cyan()
            );
        }
    }

    Ok(())
}

fn build_config(cli: &Cli) -> Result<config::Config> {
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

    let password = cli.password.as_ref().ok_or_else(|| {
        RnuError::ConfigError("Password must be specified via --password".to_string())
    })?;

    ConfigResolver::resolve(
        cli.server.as_deref(),
        cli.user.as_deref(),
        password,
        cli.repository.as_deref(),
        cli.profile.as_deref(),
        cli.verbose,
        cli.dry_run,
        &config_file,
    )
}
