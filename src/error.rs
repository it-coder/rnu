use thiserror::Error;

/// Central error type for the application.
///
/// Variants with `#[from]` automatically chain the source error (accessible via
/// `std::error::Error::source()`).  String-based variants represent validation
/// or user-facing messages where there is no underlying error to chain.
/// When adding a new variant that wraps an existing error, prefer `#[from]` or
/// `#[source]` so that error-chain tooling (`anyhow`/`eyre`/`miette`) can show
/// the full causal chain.
#[derive(Error, Debug)]
pub enum RnuError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Invalid package: {0}")]
    InvalidPackage(String),

    #[error("Upload failed: {0}")]
    UploadError(String),

    #[error("Authentication failed: {0}")]
    AuthenticationError(String),

    #[error("Network error: {0}")]
    NetworkError(String),
}

pub type Result<T> = std::result::Result<T, RnuError>;

impl RnuError {
    /// Convenience constructor for `UploadError` that preserves the
    /// underlying error as contextual information in the message.
    #[allow(dead_code)]
    pub fn upload_failed(
        msg: impl Into<String>,
        source: &(impl std::error::Error + ?Sized),
    ) -> Self {
        RnuError::UploadError(format!("{}: {}", msg.into(), source))
    }
}

impl From<std::path::StripPrefixError> for RnuError {
    fn from(error: std::path::StripPrefixError) -> Self {
        RnuError::IoError(std::io::Error::new(std::io::ErrorKind::Other, error))
    }
}

impl From<quick_xml::Error> for RnuError {
    fn from(error: quick_xml::Error) -> Self {
        RnuError::InvalidPackage(format!("XML parse error: {}", error))
    }
}
