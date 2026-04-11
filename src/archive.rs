use crate::error::{AppError, ErrorCode, io_error, sync_failed};
use crate::paths::ResolvedPaths;
use camino::Utf8PathBuf;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredFile {
    pub thread_id: String,
    pub path: Utf8PathBuf,
    pub archived: bool,
    pub size: i64,
    pub mtime_ns: i64,
}

#[derive(Debug, Clone)]
pub struct TitleInfo {
    pub thread_name: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArchiveInventory {
    pub files: Vec<DiscoveredFile>,
    pub titles: BTreeMap<String, TitleInfo>,
    pub session_index_mtime_ns: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct IndexedThread {
    pub thread_id: String,
    pub project_slug: Option<String>,
    pub project_cwd: Option<String>,
    pub path: Utf8PathBuf,
    pub archived: bool,
    pub default_scope: bool,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<String>,
    pub source_kind: String,
    pub cwd: Option<String>,
    pub cli_version: Option<String>,
    pub has_subagents: bool,
    pub message_count: i64,
    pub event_count: i64,
    pub search_text: String,
}

#[derive(Debug, Clone)]
pub struct IndexedMessage {
    pub message_id: String,
    pub thread_id: String,
    pub ordinal: i64,
    pub turn_id: Option<String>,
    pub role: String,
    pub kind: String,
    pub timestamp: Option<String>,
    pub text: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct IndexedEvent {
    pub event_id: String,
    pub thread_id: String,
    pub ordinal: i64,
    pub timestamp: Option<String>,
    pub record_type: String,
    pub payload_type: Option<String>,
    pub file_path: Utf8PathBuf,
    pub byte_start: i64,
    pub byte_len: i64,
}

#[derive(Debug, Clone)]
pub struct ParsedThread {
    pub thread: IndexedThread,
    pub messages: Vec<IndexedMessage>,
    pub events: Vec<IndexedEvent>,
}

pub fn discover_archives(paths: &ResolvedPaths) -> Result<ArchiveInventory, AppError> {
    let mut files = Vec::new();
    let sessions_exists = paths.sessions_root.exists();
    let archived_exists = paths.archived_root.exists();

    if !sessions_exists && !archived_exists {
        return Err(AppError::with_details(
            ErrorCode::ArchiveNotFound,
            "could not find Codex archive roots",
            serde_json::json!({
                "sessions_root": paths.sessions_root,
                "archived_root": paths.archived_root,
            }),
        ));
    }

    if sessions_exists {
        collect_files(&paths.sessions_root, false, true, &mut files)?;
    }
    if archived_exists {
        collect_files(&paths.archived_root, true, true, &mut files)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let (titles, session_index_mtime_ns) = load_session_index(paths)?;

    Ok(ArchiveInventory {
        files,
        titles,
        session_index_mtime_ns,
    })
}

pub fn parse_thread(
    file: &DiscoveredFile,
    title_info: Option<&TitleInfo>,
) -> Result<ParsedThread, AppError> {
    let handle = File::open(&file.path)
        .map_err(|error| io_error(&format!("failed to open {}", file.path), error))?;
    let mut reader = BufReader::new(handle);

    let mut line = String::new();
    let mut offset: i64 = 0;
    let mut event_ordinal: i64 = 0;
    let mut message_ordinal: i64 = 0;
    let mut all_messages = Vec::new();
    let mut all_events = Vec::new();
    let mut primary_events = Vec::new();
    let mut tail_messages = Vec::new();
    let mut tail_events = Vec::new();

    let mut started_at: Option<String> = None;
    let mut session_cwd: Option<String> = None;
    let mut first_turn_context_cwd: Option<String> = None;
    let mut tail_turn_context_cwd: Option<String> = None;
    let mut cli_version: Option<String> = None;
    let mut source_kind = "unknown".to_string();
    let mut default_scope = true;
    let mut has_subagents = false;
    let mut related_thread_ids: Vec<String> = Vec::new();
    let mut derived_title: Option<String> = None;
    let mut foreign_session_meta_seen = false;
    let mut tail_block_started = false;

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|error| io_error(&format!("failed to read {}", file.path), error))?;
        if bytes_read == 0 {
            break;
        }

        let byte_start = offset;
        offset += bytes_read as i64;
        let trimmed = line.trim_end_matches(['\n', '\r']);
        event_ordinal += 1;

        let value: Value = serde_json::from_str(trimmed).map_err(|error| {
            sync_failed(format!(
                "failed to parse JSONL in {} at event {}: {error}",
                file.path, event_ordinal
            ))
        })?;

        let record_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let payload = value.get("payload");
        let payload_type = payload
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        if record_type == "session_meta" {
            let payload_thread_id = payload
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if let Some(thread_id) = payload_thread_id.clone() {
                related_thread_ids.push(thread_id.clone());
                if thread_id != file.thread_id {
                    has_subagents = true;
                }
            }

            let applies_to_primary = payload_thread_id
                .as_deref()
                .map(|thread_id| thread_id == file.thread_id)
                .unwrap_or(true);
            if applies_to_primary {
                if started_at.is_none() {
                    started_at = payload
                        .and_then(|item| item.get("timestamp"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if session_cwd.is_none() {
                    session_cwd = payload
                        .and_then(|item| item.get("cwd"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if cli_version.is_none() {
                    cli_version = payload
                        .and_then(|item| item.get("cli_version"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if let Some(source) = payload.and_then(|item| item.get("source")) {
                    let (kind, is_default_scope) = classify_source(source);
                    source_kind = kind;
                    default_scope = is_default_scope;
                    if !is_default_scope {
                        has_subagents = true;
                    }
                }
            } else {
                has_subagents = true;
                foreign_session_meta_seen = true;
            }

            if applies_to_primary {
                primary_events.push(IndexedEvent {
                    event_id: format!("{}:e:{event_ordinal}", file.thread_id),
                    thread_id: file.thread_id.clone(),
                    ordinal: event_ordinal,
                    timestamp,
                    record_type,
                    payload_type,
                    file_path: file.path.clone(),
                    byte_start,
                    byte_len: bytes_read as i64,
                });
            }
            continue;
        }

        if starts_new_turn_block(&record_type, payload) {
            tail_block_started = true;
            tail_messages.clear();
            tail_events.clear();
            tail_turn_context_cwd = None;
        }

        if record_type == "turn_context" {
            let turn_context_cwd = payload
                .and_then(|item| item.get("cwd"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if first_turn_context_cwd.is_none() {
                first_turn_context_cwd = turn_context_cwd.clone();
            }
            if tail_block_started && tail_turn_context_cwd.is_none() {
                tail_turn_context_cwd = turn_context_cwd;
            }
        }

        if let Some(message) = extract_message(
            &value,
            &record_type,
            payload,
            timestamp.clone(),
            &file.thread_id,
            message_ordinal + 1,
        ) {
            if derived_title.is_none() && message.role == "user" {
                derived_title = Some(snippet_from_text(&message.text, 72));
            }
            message_ordinal += 1;
            all_messages.push(message.clone());
            if tail_block_started {
                tail_messages.push(message);
            }
        }

        let event = IndexedEvent {
            event_id: format!("{}:e:{event_ordinal}", file.thread_id),
            thread_id: file.thread_id.clone(),
            ordinal: event_ordinal,
            timestamp,
            record_type,
            payload_type,
            file_path: file.path.clone(),
            byte_start,
            byte_len: bytes_read as i64,
        };
        all_events.push(event.clone());
        if tail_block_started {
            tail_events.push(event);
        }
    }

    let use_tail_scope = foreign_session_meta_seen && !default_scope && tail_block_started;
    let messages = if use_tail_scope {
        tail_messages
    } else {
        all_messages
    };
    let mut events = primary_events;
    if use_tail_scope {
        events.extend(tail_events);
    } else {
        events.extend(all_events);
    }

    let title = title_info
        .map(|item| item.thread_name.clone())
        .or(derived_title)
        .or_else(|| Some(file.thread_id.clone()));
    let updated_at = title_info
        .and_then(|item| item.updated_at.clone())
        .or_else(|| started_at.clone());
    let search_text = build_thread_search_text(title.as_deref(), &messages);
    let project_cwd = if use_tail_scope {
        session_cwd.or(tail_turn_context_cwd)
    } else {
        session_cwd.or(first_turn_context_cwd)
    }
    .map(|cwd| normalize_project_cwd(&cwd));
    let project_slug = project_cwd.as_deref().map(project_slug_from_cwd);
    let cwd = project_cwd.clone();

    Ok(ParsedThread {
        thread: IndexedThread {
            thread_id: file.thread_id.clone(),
            project_slug,
            project_cwd,
            path: file.path.clone(),
            archived: file.archived,
            default_scope,
            title,
            updated_at,
            started_at,
            source_kind,
            cwd,
            cli_version,
            has_subagents: has_subagents || related_thread_ids.len() > 1,
            message_count: messages.len() as i64,
            event_count: events.len() as i64,
            search_text,
        },
        messages,
        events,
    })
}

fn collect_files(
    root: &Utf8PathBuf,
    archived: bool,
    recursive: bool,
    files: &mut Vec<DiscoveredFile>,
) -> Result<(), AppError> {
    let mut walker = WalkDir::new(root);
    if !recursive {
        walker = walker.max_depth(1);
    }
    for entry in walker.into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|item| item.to_str()) != Some("jsonl") {
            continue;
        }
        let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf()).map_err(|_| {
            AppError::new(
                ErrorCode::InternalError,
                format!(
                    "archive path is not valid UTF-8: {}",
                    entry.path().display()
                ),
            )
        })?;
        let metadata = entry
            .metadata()
            .map_err(|error| io_error(&format!("failed to stat {path}"), error.into()))?;
        let mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos() as i64)
            .unwrap_or_default();
        let size = metadata.len() as i64;
        files.push(DiscoveredFile {
            thread_id: thread_id_from_filename(&path),
            path,
            archived,
            size,
            mtime_ns,
        });
    }
    Ok(())
}

fn load_session_index(
    paths: &ResolvedPaths,
) -> Result<(BTreeMap<String, TitleInfo>, Option<i64>), AppError> {
    if !paths.session_index_path.exists() {
        return Ok((BTreeMap::new(), None));
    }

    let metadata = std::fs::metadata(&paths.session_index_path)
        .map_err(|error| io_error("failed to stat session_index.jsonl", error))?;
    let mtime_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos() as i64);

    let file = File::open(&paths.session_index_path)
        .map_err(|error| io_error("failed to open session_index.jsonl", error))?;
    let reader = BufReader::new(file);
    let mut titles = BTreeMap::new();
    for (line_number, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| io_error("failed to read session_index.jsonl", error))?;
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                eprintln!(
                    "warning: ignoring malformed session_index.jsonl line {}: {}",
                    line_number + 1,
                    error
                );
                continue;
            }
        };
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        let thread_name = value
            .get("thread_name")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_string();
        let updated_at = value
            .get("updated_at")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        titles.insert(
            id.to_string(),
            TitleInfo {
                thread_name,
                updated_at,
            },
        );
    }
    Ok((titles, mtime_ns))
}

fn starts_new_turn_block(record_type: &str, payload: Option<&Value>) -> bool {
    if record_type == "turn_context" {
        return true;
    }
    record_type == "event_msg"
        && payload
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("task_started")
        && payload
            .and_then(|item| item.get("turn_id"))
            .and_then(Value::as_str)
            .is_some()
}

fn classify_source(value: &Value) -> (String, bool) {
    if let Some(source) = value.as_str() {
        return (source.to_string(), true);
    }
    if let Some(subagent) = value.get("subagent") {
        if let Some(kind) = subagent.as_str() {
            return (format!("subagent:{kind}"), false);
        }
        return ("subagent".to_string(), false);
    }
    ("object".to_string(), false)
}

fn normalize_project_cwd(raw: &str) -> String {
    let mut normalized = raw.to_string();
    while normalized.len() > 1
        && (normalized.ends_with('/') || normalized.ends_with('\\'))
        && !is_windows_drive_root(&normalized)
    {
        normalized.pop();
    }
    normalized
}

fn is_windows_drive_root(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 3
        && bytes[1] == b':'
        && bytes[0].is_ascii_alphabetic()
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn project_slug_from_cwd(cwd: &str) -> String {
    let mut slug = String::with_capacity(cwd.len());
    for byte in cwd.bytes() {
        if byte.is_ascii_alphanumeric() {
            slug.push(byte as char);
            continue;
        }
        slug.push('~');
        slug.push(hex_digit(byte >> 4));
        slug.push(hex_digit(byte & 0x0f));
    }
    slug
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + (value - 10)) as char,
        _ => unreachable!("nibble out of range"),
    }
}

fn extract_message(
    value: &Value,
    record_type: &str,
    payload: Option<&Value>,
    timestamp: Option<String>,
    thread_id: &str,
    ordinal: i64,
) -> Option<IndexedMessage> {
    match record_type {
        "event_msg" => {
            let payload = payload?;
            match payload.get("type").and_then(Value::as_str) {
                Some("user_message") => {
                    let text = payload
                        .get("message")
                        .and_then(Value::as_str)?
                        .trim()
                        .to_string();
                    if text.is_empty() {
                        return None;
                    }
                    Some(IndexedMessage {
                        message_id: format!("{thread_id}:m:{ordinal}"),
                        thread_id: thread_id.to_string(),
                        ordinal,
                        turn_id: payload
                            .get("turn_id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        role: "user".to_string(),
                        kind: "user_message".to_string(),
                        timestamp,
                        snippet: snippet_from_text(&text, 140),
                        text,
                    })
                }
                Some("agent_message") => {
                    let text = payload
                        .get("message")
                        .and_then(Value::as_str)?
                        .trim()
                        .to_string();
                    if text.is_empty() {
                        return None;
                    }
                    Some(IndexedMessage {
                        message_id: format!("{thread_id}:m:{ordinal}"),
                        thread_id: thread_id.to_string(),
                        ordinal,
                        turn_id: payload
                            .get("turn_id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        role: "assistant".to_string(),
                        kind: "agent_message".to_string(),
                        timestamp,
                        snippet: snippet_from_text(&text, 140),
                        text,
                    })
                }
                _ => None,
            }
        }
        "response_item" => {
            let payload = payload?;
            if payload.get("type").and_then(Value::as_str) != Some("message") {
                return None;
            }
            if payload.get("role").and_then(Value::as_str) != Some("assistant") {
                return None;
            }
            let text = extract_message_text(payload.get("content")?)?;
            let text = text.trim().to_string();
            if text.is_empty() {
                return None;
            }
            Some(IndexedMessage {
                message_id: format!("{thread_id}:m:{ordinal}"),
                thread_id: thread_id.to_string(),
                ordinal,
                turn_id: None,
                role: "assistant".to_string(),
                kind: "assistant_message".to_string(),
                timestamp,
                snippet: snippet_from_text(&text, 140),
                text,
            })
        }
        _ => {
            let _ = value;
            None
        }
    }
}

fn extract_message_text(content: &Value) -> Option<String> {
    let array = content.as_array()?;
    let mut parts = Vec::new();
    for item in array {
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                parts.push(text.trim());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn build_thread_search_text(title: Option<&str>, messages: &[IndexedMessage]) -> String {
    let mut search = String::new();
    if let Some(title) = title {
        search.push_str(title);
        search.push('\n');
    }
    for message in messages.iter().take(48) {
        search.push_str(&message.text);
        search.push('\n');
        if search.len() > 64_000 {
            break;
        }
    }
    search
}

pub fn snippet_from_text(text: &str, limit: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let char_count = normalized.chars().count();
    if char_count <= limit {
        return normalized;
    }
    let take_count = limit.saturating_sub(3);
    let clipped = normalized.chars().take(take_count).collect::<String>();
    format!("{clipped}...")
}

fn thread_id_from_filename(path: &Utf8PathBuf) -> String {
    let stem = path.file_stem().unwrap_or_default();
    if stem.len() >= 36 {
        return stem[stem.len() - 36..].to_string();
    }
    stem.to_string()
}
