mod cli;
mod config;
mod error;
mod mvn;
mod nexus;
mod utils;

use clap::Parser;
use cli::{Cli, Commands, ConfigAction, LayoutArg};
use colored::*;
use config::{
    create_sample_config, get_default_config_file, ConfigFile, ConfigResolver, Kind, ProfileConfig,
    ResolveInput,
};
use error::{Result, RnuError};
use nexus::{FilterOptions, MavenLayout, NexusUploader};
use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    if let Err(e) = run(cli).await {
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
    match &cli.command {
        Commands::Config { action } => handle_config_command(action, &cli)?,

        Commands::Npm { path, include_dev } => {
            if *include_dev {
                warn!("--include-dev is ignored: node_modules is always scanned in full");
                eprintln!(
                    "{} --include-dev has no effect: every package under node_modules is uploaded",
                    "!".yellow().bold()
                );
            }

            let config = build_config(&cli, Kind::Npm)?;
            config.validate()?;

            info!("Starting RNU - npm upload");
            info!(server = %config.server, repository = %config.repository, dry_run = config.dry_run);

            let uploader = Arc::new(NexusUploader::new(config)?);
            let filter_options =
                FilterOptions::from_cli(cli.only.as_deref(), cli.exclude.as_deref());

            uploader
                .run(
                    path.clone(),
                    cli.skip_existing,
                    false,
                    filter_options,
                    cli.concurrency,
                )
                .await?;
        }

        Commands::Maven {
            path,
            layout,
            with_project,
            keep_repo,
            fresh,
            mvn,
        } => {
            let layout = resolve_layout(path, *layout)?;
            // --mvn is harmless for non-project layouts (and may come from
            // RNU_MVN), so only the options that change behaviour are rejected.
            if layout != MavenLayout::Project && (*with_project || keep_repo.is_some() || *fresh) {
                return Err(RnuError::ConfigError(
                    "--with-project / --keep-repo / --fresh only apply to a project source \
                     (a directory containing pom.xml, or --layout project)"
                        .to_string(),
                ));
            }

            let dir = MavenLayout::base_dir(path);
            println!(
                "{} {} ({})",
                "Source:".dimmed(),
                dir.display(),
                layout.as_str()
            );

            let config = build_config(&cli, Kind::Maven)?;
            config.validate()?;

            info!("Starting RNU - Maven upload");
            info!(server = %config.server, repository = %config.repository, dry_run = config.dry_run);

            let uploader = Arc::new(NexusUploader::new(config)?);
            let filter_options =
                FilterOptions::from_cli(cli.only.as_deref(), cli.exclude.as_deref());

            match layout {
                MavenLayout::Project => {
                    uploader
                        .run_maven_project(
                            dir,
                            *with_project,
                            keep_repo.clone(),
                            *fresh,
                            mvn.clone(),
                            cli.skip_existing,
                            filter_options,
                            cli.concurrency,
                        )
                        .await?;
                }
                MavenLayout::Repo => {
                    uploader
                        .run_maven_repo(dir, cli.skip_existing, filter_options, cli.concurrency)
                        .await?;
                }
                MavenLayout::Loose => {
                    uploader
                        .run_maven(dir, cli.skip_existing, filter_options, cli.concurrency)
                        .await?;
                }
            }
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

/// Turn the `--layout` choice into a concrete layout, validating the path.
fn resolve_layout(path: &Path, requested: LayoutArg) -> Result<MavenLayout> {
    if !path.exists() {
        return Err(RnuError::PackageNotFound(format!(
            "Path not found: {}",
            path.display()
        )));
    }
    Ok(match requested {
        LayoutArg::Auto => MavenLayout::detect(path)?,
        LayoutArg::Project => MavenLayout::Project,
        LayoutArg::Repo => MavenLayout::Repo,
        LayoutArg::Loose => MavenLayout::Loose,
    })
}

fn build_config(cli: &Cli, kind: Kind) -> Result<config::Config> {
    let path = config_path(cli)?;
    let config_file = ConfigFile::load_from_file(&path)?;
    let input = ResolveInput {
        server: cli.server.as_deref(),
        user: cli.user.as_deref(),
        password: cli.password.as_deref(),
        repository: cli.repository.as_deref(),
        profile: cli.profile.as_deref(),
        verbose: cli.verbose,
        dry_run: cli.dry_run,
    };
    let resolved = ConfigResolver::resolve(kind, &input, &config_file);

    // Only mention the old global config when the run actually needs one and
    // there is none here — otherwise every command would nag about it.
    if resolved.is_err() && cli.config.is_none() && !path.exists() {
        if let Some(legacy) = config::legacy_config_file() {
            eprintln!(
                "{} Legacy config at {} is no longer read — the config file now lives in the \
                 current directory ({}). Copy it over with:\n  nexus-upload config init --from \"{}\"",
                "!".yellow().bold(),
                legacy.display(),
                config::CONFIG_FILE_NAME,
                legacy.display()
            );
        }
    }

    resolved
}

fn config_path(cli: &Cli) -> Result<PathBuf> {
    match &cli.config {
        Some(path) => Ok(path.clone()),
        None => get_default_config_file(),
    }
}

// ── config subcommand ─────────────────────────────────────────────

fn handle_config_command(action: &ConfigAction, cli: &Cli) -> Result<()> {
    let path = config_path(cli)?;

    match action {
        ConfigAction::Init { force, from } => config_init(&path, *force, from.as_deref(), cli),
        ConfigAction::Show => config_show(&path),
        ConfigAction::List => config_list(&path),
        ConfigAction::Add {
            name,
            npm_repo,
            maven_repo,
        } => config_add(
            &path,
            cli,
            name.clone(),
            npm_repo.clone(),
            maven_repo.clone(),
        ),
    }
}

fn config_init(path: &Path, force: bool, from: Option<&Path>, cli: &Cli) -> Result<()> {
    if path.exists() && !force {
        return Err(RnuError::ConfigError(format!(
            "Configuration file already exists at {}. Use --force to overwrite.",
            path.display()
        )));
    }

    let mut cfg = match from {
        Some(source) => {
            if !source.exists() {
                return Err(RnuError::ConfigError(format!(
                    "Cannot seed from {}: file not found",
                    source.display()
                )));
            }
            ConfigFile::load_from_file(source)?
        }
        None => create_sample_config(),
    };
    let interactive = io::stdin().is_terminal() && cli.server.is_none();

    if interactive {
        println!(
            "Creating {}\nPress Enter to accept the value in brackets.\n",
            path.display()
        );

        // Seed the repo prompts from whatever the base config already had, so a
        // `--from` migration only needs Enter pressed a few times.
        let existing = cfg.repositories.clone().unwrap_or_default();
        let npm_default = existing
            .get("npm")
            .cloned()
            .unwrap_or_else(|| "npm-hosted".to_string());
        let maven_default = existing
            .get("maven")
            .cloned()
            .unwrap_or_else(|| "maven-releases".to_string());

        cfg.default_server = prompt("Nexus server URL", cfg.default_server.as_deref())?;
        cfg.default_user = prompt("Username", cfg.default_user.as_deref())?;
        let npm_repo = prompt("npm repository", Some(&npm_default))?;
        let maven_repo = prompt("Maven repository", Some(&maven_default))?;
        // Asked last: rpassword reads from the terminal directly, so mix it
        // with the stdin prompts only once they are done.
        cfg.default_password =
            prompt_secret("Password (empty = use RNU_PASSWORD)")?.filter(|v| !v.is_empty());

        let mut repos = BTreeMap::new();
        if let Some(repo) = npm_repo.filter(|v| !v.is_empty()) {
            repos.insert("npm".to_string(), repo);
        }
        if let Some(repo) = maven_repo.filter(|v| !v.is_empty()) {
            repos.insert("maven".to_string(), repo);
        }
        cfg.repositories = if repos.is_empty() { None } else { Some(repos) };
        if from.is_none() {
            // Fresh init writes flat defaults, not the placeholder profiles.
            cfg.profiles = None;
        }
    } else {
        // Non-interactive: take whatever flags/env supplied, keep the rest.
        if let Some(server) = &cli.server {
            cfg.default_server = Some(server.clone());
        }
        if let Some(user) = &cli.user {
            cfg.default_user = Some(user.clone());
        }
        if let Some(password) = &cli.password {
            cfg.default_password = Some(password.clone());
        }
    }

    cfg.save_to_file(path)?;

    println!(
        "{} Configuration written to {}",
        "✓".green().bold(),
        path.display()
    );
    if cfg.default_password.is_some() {
        println!(
            "  {} Password stored in plain text (file mode 600). Prefer RNU_PASSWORD, and keep \
             {} out of version control if it holds a password.",
            "!".yellow().bold(),
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
    }
    println!(
        "Now try: {}",
        "nexus-upload maven ./my-app --dry-run".cyan()
    );
    Ok(())
}

fn config_show(path: &Path) -> Result<()> {
    if !path.exists() {
        println!(
            "{} No configuration file found at: {}",
            "!".yellow().bold(),
            path.display()
        );
        println!("Run 'nexus-upload config init' to create one.");
        return Ok(());
    }

    let cfg = mask_secrets(ConfigFile::load_from_file(path)?);
    println!("Configuration file: {}", path.display());
    println!("{}", cfg.render(path)?);
    Ok(())
}

fn config_list(path: &Path) -> Result<()> {
    if !path.exists() {
        println!("{} No configuration file found.", "!".yellow().bold());
        return Ok(());
    }

    let cfg = ConfigFile::load_from_file(path)?;
    println!("{}", "Default:".bold());
    println!("  server: {}", cfg.default_server.as_deref().unwrap_or("-"));
    println!("  user:   {}", cfg.default_user.as_deref().unwrap_or("-"));
    for (kind, repo) in sorted_repos(cfg.repositories.as_ref()) {
        println!("  {} repository: {}", kind, repo);
    }

    match cfg.profiles {
        Some(profiles) if !profiles.is_empty() => {
            println!("\n{}", "Profiles:".bold());
            let mut names: Vec<_> = profiles.keys().cloned().collect();
            names.sort();
            for name in names {
                let profile = &profiles[&name];
                println!("  {} - {}", name.cyan(), profile.server);
                for (kind, repo) in sorted_repos(profile.repositories.as_ref()) {
                    println!("    {} repository: {}", kind, repo);
                }
            }
        }
        _ => println!("\nNo profiles configured."),
    }
    Ok(())
}

fn config_add(
    path: &Path,
    cli: &Cli,
    name: String,
    npm_repo: Option<String>,
    maven_repo: Option<String>,
) -> Result<()> {
    let server = cli.server.clone().ok_or_else(|| {
        RnuError::ConfigError(
            "A server is required to add a profile: pass -s/--server or set RNU_SERVER".to_string(),
        )
    })?;

    let mut cfg = ConfigFile::load_from_file(path)?;

    let mut repos = BTreeMap::new();
    if let Some(repo) = npm_repo {
        repos.insert("npm".to_string(), repo);
    }
    if let Some(repo) = maven_repo {
        repos.insert("maven".to_string(), repo);
    }

    cfg.add_profile(
        name.clone(),
        ProfileConfig {
            server,
            repository: None,
            repositories: if repos.is_empty() { None } else { Some(repos) },
            user: cli.user.clone(),
            password: cli.password.clone(),
        },
    );
    cfg.save_to_file(path)?;

    println!(
        "{} Profile '{}' added to {}",
        "✓".green().bold(),
        name.cyan(),
        path.display()
    );
    if cli.password.is_some() {
        println!(
            "  {} Password stored in plain text (file mode 600). Prefer RNU_PASSWORD in CI.",
            "!".yellow().bold()
        );
    }
    Ok(())
}

fn sorted_repos(repos: Option<&BTreeMap<String, String>>) -> Vec<(String, String)> {
    repos
        .map(|r| r.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

fn mask_secrets(mut cfg: ConfigFile) -> ConfigFile {
    cfg.default_password = cfg.default_password.map(|_| "***".to_string());
    if let Some(profiles) = cfg.profiles.as_mut() {
        for profile in profiles.values_mut() {
            profile.password = profile.password.as_ref().map(|_| "***".to_string());
        }
    }
    cfg
}

fn prompt(label: &str, default: Option<&str>) -> Result<Option<String>> {
    match default {
        Some(value) => print!("{label} [{value}]: "),
        None => print!("{label}: "),
    }
    io::stdout().flush()?;

    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        return Ok(default.map(str::to_string));
    }
    let answer = line.trim();
    if answer.is_empty() {
        Ok(default.map(str::to_string))
    } else {
        Ok(Some(answer.to_string()))
    }
}

fn prompt_secret(label: &str) -> Result<Option<String>> {
    let value = rpassword::prompt_password(format!("{label}: "))?;
    Ok(Some(value))
}
