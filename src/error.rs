use serde::Serialize;
use serde_json::{Value, json};
use std::fmt::{Display, Formatter};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UsageError,
    ArchiveNotFound,
    IndexMissing,
    NotFound,
    Ambiguous,
    SyncFailed,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UsageError => "usage_error",
            Self::ArchiveNotFound => "archive_not_found",
            Self::IndexMissing => "index_missing",
            Self::NotFound => "not_found",
            Self::Ambiguous => "ambiguous",
            Self::SyncFailed => "sync_failed",
            Self::InternalError => "internal_error",
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Self::UsageError => 2,
            Self::ArchiveNotFound => 3,
            Self::IndexMissing => 4,
            Self::NotFound => 5,
            Self::Ambiguous => 6,
            Self::SyncFailed => 7,
            Self::InternalError => 1,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug)]
pub struct AppError {
    code: ErrorCode,
    message: String,
    details: Option<Value>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(
        code: ErrorCode,
        message: impl Into<String>,
        details: impl Into<Value>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn exit_code(&self) -> i32 {
        self.code.exit_code()
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            code: self.code.as_str(),
            message: self.message.clone(),
            details: self.details.clone(),
        }
    }

    pub fn is_sqlite_locked(&self) -> bool {
        self.details
            .as_ref()
            .and_then(|details| details.get("sqlite_error"))
            .and_then(Value::as_str)
            .map(|value| value.contains("database is locked"))
            .unwrap_or(false)
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

pub fn internal(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InternalError, message)
}

pub fn sync_failed(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::SyncFailed, message)
}

pub fn io_error(context: &str, error: std::io::Error) -> AppError {
    AppError::with_details(
        ErrorCode::InternalError,
        format!("{context}: {error}"),
        json!({ "os_error": error.to_string() }),
    )
}
