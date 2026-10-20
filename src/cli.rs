use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "nexus-upload",
    version = "0.2.0",
    about = "Rust Nexus Uploader - Upload packages to Nexus repository",
    long_about = "A CLI tool for uploading npm packages to Nexus repository with offline support"
)]
pub struct Cli {
    /// Nexus server URL (e.g., http://nexus.example.com:8081)
    #[arg(long, short = 's', help = "Nexus server URL")]
    pub server: Option<String>,

    /// Username for Nexus authentication
    #[arg(long, short = 'u', help = "Username for authentication")]
    pub user: Option<String>,

    /// Password for Nexus authentication
    #[arg(long, short = 'p', help = "Password for authentication")]
    pub password: Option<String>,

    /// Repository name in Nexus (defaults to npm-hosted)
    #[arg(long, short = 'r', help = "Repository name")]
    pub repository: Option<String>,

    /// Use a specific profile from config file
    #[arg(long, help = "Use configuration profile")]
    pub profile: Option<String>,

    /// Configuration file path
    #[arg(long, help = "Path to configuration file")]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(long, short = 'v', help = "Enable verbose logging")]
    pub verbose: bool,

    /// Dry run mode (don't actually upload)
    #[arg(
        long,
        default_value_t = false,
        help = "Dry run - show what would be uploaded without uploading"
    )]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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

        /// Number of concurrent uploads
        #[arg(
            long,
            short = 'c',
            default_value = "4",
            help = "Number of concurrent uploads"
        )]
        concurrency: usize,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Upload Maven packages to Nexus repository
    Maven {
        /// Path to directory containing POM/JAR/WAR files
        #[arg(help = "Path to directory with Maven artifacts")]
        path: PathBuf,

        /// Skip packages that already exist in repository
        #[arg(long, help = "Skip packages that already exist")]
        skip_existing: bool,

        /// Only upload specific artifacts (comma-separated)
        #[arg(long, help = "Only upload specific artifacts (comma-separated)")]
        only: Option<String>,

        /// Exclude specific artifacts (comma-separated)
        #[arg(long, help = "Exclude specific artifacts (comma-separated)")]
        exclude: Option<String>,

        /// Number of concurrent uploads
        #[arg(
            long,
            short = 'c',
            default_value = "4",
            help = "Number of concurrent uploads"
        )]
        concurrency: usize,
    },

    #[command(hide = true)]
    Pypi {
        path: PathBuf,
        #[arg(long)]
        skip_existing: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
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
