use crate::error::{AppError, ErrorBody};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Debug, Serialize)]
pub struct Meta {
    pub generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_sync_performed: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub schema_version: u32,
    pub command: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    pub meta: Meta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl<T: Serialize> Envelope<T> {
    pub fn success(command: impl Into<String>, data: T, auto_sync_performed: Option<bool>) -> Self {
        Self {
            schema_version: 1,
            command: command.into(),
            ok: true,
            data: Some(data),
            meta: Meta {
                generated_at: now_rfc3339(),
                auto_sync_performed,
            },
            error: None,
        }
    }
}

impl Envelope<serde_json::Value> {
    pub fn failure(
        command: impl Into<String>,
        error: &AppError,
        auto_sync_performed: Option<bool>,
    ) -> Self {
        Self {
            schema_version: 1,
            command: command.into(),
            ok: false,
            data: None,
            meta: Meta {
                generated_at: now_rfc3339(),
                auto_sync_performed,
            },
            error: Some(error.body()),
        }
    }
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
