use crate::error::{AppError, ErrorCode, internal, io_error};
use camino::Utf8PathBuf;
use serde::Serialize;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPaths {
    pub codex_home: Utf8PathBuf,
    pub sessions_root: Utf8PathBuf,
    pub archived_root: Utf8PathBuf,
    pub session_index_path: Utf8PathBuf,
    pub index_dir: Utf8PathBuf,
    pub index_path: Utf8PathBuf,
}

impl ResolvedPaths {
    pub fn discover() -> Result<Self, AppError> {
        let codex_home = if let Some(value) = env::var_os("CODEX_HOME") {
            utf8_from_path(PathBuf::from(value), "CODEX_HOME")?
        } else {
            let home = env::var_os("HOME")
                .ok_or_else(|| AppError::new(ErrorCode::ArchiveNotFound, "HOME is not set"))?;
            let mut path = PathBuf::from(home);
            path.push(".codex");
            utf8_from_path(path, "HOME/.codex")?
        };

        let sessions_root = codex_home.join("sessions");
        let archived_root = codex_home.join("archived_sessions");
        let session_index_path = codex_home.join("session_index.jsonl");
        let index_dir = codex_home.join("codex-threads");
        let index_path = index_dir.join("index.sqlite");

        Ok(Self {
            codex_home,
            sessions_root,
            archived_root,
            session_index_path,
            index_dir,
            index_path,
        })
    }

    pub fn ensure_index_dir(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.index_dir)
            .map_err(|error| io_error("failed to create index directory", error))
    }
}

fn utf8_from_path(path: PathBuf, label: &str) -> Result<Utf8PathBuf, AppError> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|_| internal(format!("resolved {label} path is not valid UTF-8")))
}
