use std::fmt;

#[derive(Debug)]
pub enum RnuError {
    IoError(std::io::Error),
    JsonError(serde_json::Error),
    HttpError(reqwest::Error),
    ConfigError(String),
    PackageNotFound(String),
    InvalidPackage(String),
    UploadError(String),
    AuthenticationError(String),
    NetworkError(String),
}

impl fmt::Display for RnuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RnuError::IoError(e) => write!(f, "IO error: {}", e),
            RnuError::JsonError(e) => write!(f, "JSON parsing error: {}", e),
            RnuError::HttpError(e) => write!(f, "HTTP request error: {}", e),
            RnuError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            RnuError::PackageNotFound(name) => write!(f, "Package not found: {}", name),
            RnuError::InvalidPackage(msg) => write!(f, "Invalid package: {}", msg),
            RnuError::UploadError(msg) => write!(f, "Upload failed: {}", msg),
            RnuError::AuthenticationError(msg) => write!(f, "Authentication failed: {}", msg),
            RnuError::NetworkError(msg) => write!(f, "Network error: {}", msg),
        }
    }
}

impl std::error::Error for RnuError {}

impl From<std::io::Error> for RnuError {
    fn from(error: std::io::Error) -> Self {
        RnuError::IoError(error)
    }
}

impl From<serde_json::Error> for RnuError {
    fn from(error: serde_json::Error) -> Self {
        RnuError::JsonError(error)
    }
}

impl From<reqwest::Error> for RnuError {
    fn from(error: reqwest::Error) -> Self {
        RnuError::HttpError(error)
    }
}

pub type Result<T> = std::result::Result<T, RnuError>;

impl From<std::path::StripPrefixError> for RnuError {
    fn from(error: std::path::StripPrefixError) -> Self {
        RnuError::IoError(std::io::Error::new(std::io::ErrorKind::Other, error))
    }
}
