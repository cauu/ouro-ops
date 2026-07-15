use thiserror::Error;

pub type Result<T> = std::result::Result<T, OuroError>;

#[derive(Debug, Error)]
pub enum OuroError {
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The command already emitted its single typed failure record; main must only preserve the
    /// non-zero exit code and must not print a second JSON line.
    #[error("command failure already reported")]
    Reported(i32),
}

impl OuroError {
    pub fn exit_code(&self) -> i32 {
        match self {
            OuroError::InvalidArgs(_) | OuroError::Validation(_) => 10,
            OuroError::Io(_) | OuroError::Json(_) | OuroError::Yaml(_) | OuroError::Sqlite(_) => 20,
            OuroError::Reported(code) => *code,
        }
    }

    pub fn is_reported(&self) -> bool {
        matches!(self, OuroError::Reported(_))
    }
}
