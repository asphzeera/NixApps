use std::path::PathBuf;
use thiserror::Error;

/// Unified error type for `nixapps-core`.
#[derive(Debug, Error)]
pub enum NixAppsError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid channel name '{0}': only alphanumeric, '.', '-', and '_' are allowed")]
    InvalidChannel(String),

    #[error("invalid Nix attribute path '{0}'")]
    InvalidAttrPath(String),

    #[error("generated Nix module failed syntax validation:\n{stderr}")]
    NixValidationFailed { stderr: String },

    #[error(
        "refusing to overwrite unmanaged file '{path}': file exists without the NixApps header"
    )]
    UnmanagedFileExists { path: PathBuf },

    #[error("git command failed: {message}")]
    Git { message: String },

    #[error("sync failed: {0}")]
    Sync(String),
}

pub type Result<T> = std::result::Result<T, NixAppsError>;
