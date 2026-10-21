use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "nexus-upload",
    version,
    about = "Batch-upload npm and Maven packages into a Nexus repository",
    long_about = "Batch-upload npm and Maven packages into a Nexus repository.\n\n\
                  Connection settings are resolved in this order (highest first):\n\
                  command-line flags > environment variables > --profile > config file.\n\
                  Environment variables: RNU_SERVER, RNU_USER, RNU_PASSWORD, RNU_REPO\n\n\
                  The config file is ./config.toml in the current directory, so each\n\
                  project can keep its own settings. Use --config to point elsewhere.",
    after_help = "Examples:\n  \
                  nexus-upload config init\n  \
                  nexus-upload maven ./my-app\n  \
                  nexus-upload maven ~/.m2/repository --skip-existing\n  \
                  nexus-upload npm ./my-app --dry-run\n\n\
                  Global options may be written before or after the subcommand."
)]
pub struct Cli {
    /// Nexus server URL, e.g. http://nexus.example.com:8081
    #[arg(
        help_heading = "Connection",
        long,
        short = 's',
        global = true,
        env = "RNU_SERVER",
        value_name = "URL"
    )]
    pub server: Option<String>,

    /// Username for Nexus authentication
    #[arg(
        help_heading = "Connection",
        long,
        short = 'u',
        global = true,
        env = "RNU_USER",
        value_name = "USER"
    )]
    pub user: Option<String>,

    /// Password for Nexus authentication
    #[arg(
        help_heading = "Connection",
        long,
        short = 'p',
        global = true,
        env = "RNU_PASSWORD",
        hide_env_values = true,
        value_name = "PASSWORD"
    )]
    pub password: Option<String>,

    /// Target repository name
    #[arg(
        help_heading = "Connection",
        long,
        short = 'r',
        global = true,
        env = "RNU_REPO",
        value_name = "REPO"
    )]
    pub repository: Option<String>,

    /// Use a named profile from the config file
    #[arg(help_heading = "Connection", long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// Path to the config file (default: ./config.toml; a *.json path is still read as JSON)
    #[arg(help_heading = "Connection", long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(help_heading = "Logging", long, short = 'v', global = true)]
    pub verbose: bool,

    /// Show what would be uploaded without uploading
    #[arg(help_heading = "Upload", long, global = true)]
    pub dry_run: bool,

    /// Number of concurrent uploads
    #[arg(
        help_heading = "Upload",
        long,
        short = 'c',
        global = true,
        default_value_t = 4,
        value_name = "N"
    )]
    pub concurrency: usize,

    /// Skip packages that already exist in the repository
    #[arg(help_heading = "Upload", long, global = true)]
    pub skip_existing: bool,

    /// Only upload packages matching these names (comma-separated)
    #[arg(help_heading = "Upload", long, global = true, value_name = "LIST")]
    pub only: Option<String>,

    /// Exclude packages matching these names (comma-separated)
    #[arg(help_heading = "Upload", long, global = true, value_name = "LIST")]
    pub exclude: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Upload every package found under node_modules
    Npm {
        /// Project directory or node_modules directory
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Deprecated: node_modules is always scanned in full, this flag has no effect
        #[arg(long, hide = true)]
        include_dev: bool,
    },

    /// Upload Maven artifacts from a project, a repository layout, or loose POM files
    #[command(after_help = "PATH is interpreted automatically:\n  \
        contains pom.xml          -> resolve the dependency closure with mvn\n  \
        <group>/<artifact>/<ver>  -> scan a Maven repository layout\n  \
        otherwise                 -> scan loose .pom files\n\n\
        Use --layout to override the detection.")]
    Maven {
        /// Project directory (or pom.xml), Maven repository layout, or directory of POM/JAR files
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Interpret PATH as this instead of detecting it
        #[arg(long, value_enum, default_value_t = LayoutArg::Auto, value_name = "KIND")]
        layout: LayoutArg,

        /// Project mode: also upload the project's own modules (mvn install)
        #[arg(long)]
        with_project: bool,

        /// Project mode: reuse a persistent repository directory instead of the cache
        #[arg(long, value_name = "DIR")]
        keep_repo: Option<PathBuf>,

        /// Project mode: ignore the cached repository and resolve from scratch
        #[arg(long)]
        fresh: bool,

        /// Project mode: mvn executable to use
        #[arg(long, value_name = "PATH", env = "RNU_MVN")]
        mvn: Option<PathBuf>,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    #[command(hide = true)]
    Pypi {
        path: PathBuf,
        #[arg(long)]
        skip_existing: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LayoutArg {
    /// Detect from the directory contents
    Auto,
    /// A project directory: resolve dependencies with mvn
    Project,
    /// A Maven repository layout (group/artifact/version/...)
    Repo,
    /// A directory of loose .pom / .jar files
    Loose,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Create the config file (interactive when run in a terminal)
    Init {
        /// Overwrite an existing config file
        #[arg(long)]
        force: bool,
        /// Seed the new file from an existing one (TOML or JSON)
        #[arg(long, value_name = "PATH")]
        from: Option<PathBuf>,
    },
    /// Show the current configuration (password masked)
    Show,
    /// List available profiles
    List,
    /// Add or replace a profile
    ///
    /// The profile takes its server, user and password from the global flags,
    /// e.g. `nexus-upload -s http://nexus:8081 -u deployer config add corp`.
    Add {
        /// Profile name
        name: String,
        /// Repository for npm uploads
        #[arg(long, value_name = "REPO")]
        npm_repo: Option<String>,
        /// Repository for Maven uploads
        #[arg(long, value_name = "REPO")]
        maven_repo: Option<String>,
    },
}
