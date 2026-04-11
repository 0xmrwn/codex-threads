use crate::archive::{
    ArchiveInventory, IndexedEvent, IndexedMessage, ParsedThread, discover_archives, parse_thread,
};
use crate::error::{AppError, ErrorCode, internal, io_error};
use crate::paths::ResolvedPaths;
use camino::Utf8PathBuf;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const SCHEMA_VERSION: &str = "2";

#[derive(Debug, Serialize)]
pub struct SyncSummary {
    pub discovered_files: usize,
    pub updated_files: usize,
    pub removed_files: usize,
    pub project_count: usize,
    pub thread_count: usize,
    pub message_count: usize,
    pub event_count: usize,
    pub rebuilt: bool,
}

#[derive(Debug, Serialize)]
pub struct ThreadRecord {
    pub thread_id: String,
    pub project_slug: Option<String>,
    pub project_cwd: Option<String>,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<String>,
    pub source_kind: String,
    pub cwd: Option<String>,
    pub cli_version: Option<String>,
    pub has_subagents: bool,
    pub message_count: i64,
    pub event_count: i64,
    pub archived: bool,
    pub default_scope: bool,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct ThreadSearchHit {
    pub thread_id: String,
    pub project_slug: Option<String>,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<String>,
    pub source_kind: String,
    pub cwd: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
pub struct MessageRecord {
    pub message_id: String,
    pub thread_id: String,
    pub project_slug: Option<String>,
    pub turn_id: Option<String>,
    pub role: String,
    pub kind: String,
    pub timestamp: Option<String>,
    pub text: String,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
pub struct MessageSearchHit {
    pub message_id: String,
    pub thread_id: String,
    pub project_slug: Option<String>,
    pub role: String,
    pub kind: String,
    pub timestamp: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
pub struct EventRecord {
    pub event_id: String,
    pub thread_id: String,
    pub ordinal: i64,
    pub timestamp: Option<String>,
    pub record_type: String,
    pub payload_type: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Serialize)]
pub struct ProjectRecord {
    pub project_slug: String,
    pub project_cwd: Option<String>,
    pub thread_count: i64,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatsRecord {
    pub index_path: String,
    pub last_sync_at: Option<String>,
    pub source_file_count: i64,
    pub project_count: i64,
    pub thread_count: i64,
    pub message_count: i64,
    pub event_count: i64,
    pub sessions_root: String,
    pub archived_root: String,
}

#[derive(Debug, Clone)]
struct FileState {
    path: String,
    thread_id: String,
    size: i64,
    mtime_ns: i64,
}

pub fn sync(paths: &ResolvedPaths, rebuild: bool) -> Result<SyncSummary, AppError> {
    let inventory = discover_archives(paths)?;
    let rebuild = rebuild || schema_requires_rebuild(paths)?;
    sync_with_inventory(paths, inventory, rebuild)
}

pub fn ensure_fresh(paths: &ResolvedPaths) -> Result<bool, AppError> {
    let inventory = discover_archives(paths)?;
    let rebuild = schema_requires_rebuild(paths)?;
    if rebuild || needs_sync(paths, &inventory)? {
        match sync_with_inventory(paths, inventory, rebuild) {
            Ok(_) => return Ok(true),
            Err(error) if !rebuild && error.is_sqlite_locked() && paths.index_path.exists() => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

pub fn list_projects(
    paths: &ResolvedPaths,
    limit: usize,
) -> Result<(Vec<ProjectRecord>, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let mut stmt = conn
        .prepare(
            "SELECT project_slug, project_cwd, thread_count, last_seen_at
             FROM projects
             ORDER BY COALESCE(last_seen_at, '') DESC, project_slug
             LIMIT ?1",
        )
        .map_err(sqlite_err)?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok(ProjectRecord {
                project_slug: row.get(0)?,
                project_cwd: row.get(1)?,
                thread_count: row.get(2)?,
                last_seen_at: row.get(3)?,
            })
        })
        .map_err(sqlite_err)?;
    let items = rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?;
    Ok((items, auto_sync))
}

pub fn list_threads(
    paths: &ResolvedPaths,
    limit: usize,
    project: Option<&str>,
    ascending: bool,
) -> Result<(Vec<ThreadRecord>, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let resolved_project = resolve_project_filter(&conn, project)?;
    let direction = if ascending { "ASC" } else { "DESC" };
    let sql = if resolved_project.is_some() {
        format!(
            "SELECT thread_id, project_slug, project_cwd, title, updated_at, started_at,
                    source_kind, cwd, cli_version, has_subagents, message_count, event_count,
                    archived, default_scope, path
             FROM threads
             WHERE default_scope = 1 AND project_slug = ?2
             ORDER BY CASE WHEN COALESCE(started_at, updated_at) IS NULL THEN 1 ELSE 0 END ASC,
                      COALESCE(started_at, updated_at, '') {direction},
                      thread_id ASC
             LIMIT ?1"
        )
    } else {
        format!(
            "SELECT thread_id, project_slug, project_cwd, title, updated_at, started_at,
                    source_kind, cwd, cli_version, has_subagents, message_count, event_count,
                    archived, default_scope, path
             FROM threads
             WHERE default_scope = 1
             ORDER BY CASE WHEN COALESCE(started_at, updated_at) IS NULL THEN 1 ELSE 0 END ASC,
                      COALESCE(started_at, updated_at, '') {direction},
                      thread_id ASC
             LIMIT ?1"
        )
    };

    let items = if let Some(slug) = resolved_project {
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![limit as i64, slug], thread_record_row)
            .map_err(sqlite_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
    } else {
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![limit as i64], thread_record_row)
            .map_err(sqlite_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
    };
    Ok((items, auto_sync))
}

pub fn search_threads(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
    project: Option<&str>,
) -> Result<(Vec<ThreadSearchHit>, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let resolved_project = resolve_project_filter(&conn, project)?;
    let query = fts_query(query);
    let mut sql = String::from(
        "SELECT t.thread_id, t.project_slug, t.title, t.updated_at, t.started_at,
                t.source_kind, t.cwd,
                snippet(thread_fts, 1, '', '', ' ... ', 14) AS snippet
         FROM thread_fts
         JOIN threads t USING(thread_id)
         WHERE thread_fts MATCH ?1 AND t.default_scope = 1",
    );
    if resolved_project.is_some() {
        sql.push_str(" AND t.project_slug = ?3");
    }
    sql.push_str(
        " ORDER BY bm25(thread_fts), COALESCE(t.updated_at, '') DESC, t.thread_id
          LIMIT ?2",
    );

    let hits = if let Some(slug) = resolved_project {
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![query, limit as i64, slug], thread_search_hit_row)
            .map_err(sqlite_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
    } else {
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![query, limit as i64], thread_search_hit_row)
            .map_err(sqlite_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
    };
    Ok((hits, auto_sync))
}

pub fn resolve_thread(
    paths: &ResolvedPaths,
    query: &str,
) -> Result<(ThreadRecord, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;

    if let Some(record) = read_thread_from_conn(&conn, query)? {
        return Ok((record, auto_sync));
    }

    let exact_title_matches = exact_title_matches(&conn, query)?;
    if exact_title_matches.len() == 1 {
        let record = read_thread_from_conn(&conn, &exact_title_matches[0])?
            .ok_or_else(|| internal("resolved thread disappeared"))?;
        return Ok((record, auto_sync));
    }
    if exact_title_matches.len() > 1 {
        return Err(AppError::with_details(
            ErrorCode::Ambiguous,
            format!("multiple threads exactly matched '{query}'"),
            serde_json::json!({
                "candidates": thread_hits_for_ids(&conn, &exact_title_matches)?
            }),
        ));
    }

    let candidates = search_threads_inner(&conn, query, 5)?;
    if candidates.is_empty() {
        return Err(AppError::with_details(
            ErrorCode::NotFound,
            format!("no thread matched '{query}'"),
            serde_json::json!({ "query": query }),
        ));
    }
    if candidates.len() > 1 {
        return Err(AppError::with_details(
            ErrorCode::Ambiguous,
            format!("multiple threads matched '{query}'"),
            serde_json::json!({ "candidates": candidates }),
        ));
    }
    let record = read_thread_from_conn(&conn, &candidates[0].thread_id)?
        .ok_or_else(|| internal("resolved thread disappeared"))?;
    Ok((record, auto_sync))
}

pub fn read_thread(
    paths: &ResolvedPaths,
    thread_id: &str,
) -> Result<(ThreadRecord, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let record = read_thread_from_conn(&conn, thread_id)?.ok_or_else(|| {
        AppError::with_details(
            ErrorCode::NotFound,
            format!("thread '{thread_id}' was not found"),
            serde_json::json!({ "thread_id": thread_id }),
        )
    })?;
    Ok((record, auto_sync))
}

pub fn search_messages(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
    project: Option<&str>,
) -> Result<(Vec<MessageSearchHit>, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let resolved_project = resolve_project_filter(&conn, project)?;
    let query = fts_query(query);
    let mut sql = String::from(
        "SELECT m.message_id, m.thread_id, t.project_slug, m.role, m.kind, m.timestamp,
                snippet(message_fts, 2, '', '', ' ... ', 18) AS snippet
         FROM message_fts
         JOIN messages m USING(message_id)
         JOIN threads t ON t.thread_id = m.thread_id
         WHERE message_fts MATCH ?1 AND t.default_scope = 1",
    );
    if resolved_project.is_some() {
        sql.push_str(" AND t.project_slug = ?3");
    }
    sql.push_str(
        " ORDER BY bm25(message_fts), COALESCE(m.timestamp, '') DESC, m.message_id
          LIMIT ?2",
    );

    let hits = if let Some(slug) = resolved_project {
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![query, limit as i64, slug], |row| {
                Ok(MessageSearchHit {
                    message_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    project_slug: row.get(2)?,
                    role: row.get(3)?,
                    kind: row.get(4)?,
                    timestamp: row.get(5)?,
                    snippet: row.get(6)?,
                })
            })
            .map_err(sqlite_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
    } else {
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(MessageSearchHit {
                    message_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    project_slug: row.get(2)?,
                    role: row.get(3)?,
                    kind: row.get(4)?,
                    timestamp: row.get(5)?,
                    snippet: row.get(6)?,
                })
            })
            .map_err(sqlite_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
    };
    Ok((hits, auto_sync))
}

pub fn list_messages(
    paths: &ResolvedPaths,
    limit: usize,
    project: Option<&str>,
    role: Option<String>,
    ascending: bool,
) -> Result<(Vec<MessageRecord>, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let resolved_project = resolve_project_filter(&conn, project)?;
    let direction = if ascending { "ASC" } else { "DESC" };
    let mut sql = format!(
        "SELECT m.message_id, m.thread_id, t.project_slug, m.turn_id, m.role, m.kind,
                m.timestamp, m.text, m.snippet
         FROM messages m
         JOIN threads t ON t.thread_id = m.thread_id
         WHERE t.default_scope = 1"
    );
    let mut next_param = 2;
    if resolved_project.is_some() {
        sql.push_str(&format!(" AND t.project_slug = ?{next_param}"));
        next_param += 1;
    }
    if role.is_some() {
        sql.push_str(&format!(" AND m.role = ?{next_param}"));
    }
    sql.push_str(&format!(
        " ORDER BY CASE WHEN m.timestamp IS NULL THEN 1 ELSE 0 END ASC,
                  COALESCE(m.timestamp, '') {direction},
                  m.thread_id ASC,
                  m.ordinal ASC
          LIMIT ?1"
    ));

    let items = match (resolved_project.as_deref(), role.as_deref()) {
        (Some(slug), Some(role)) => {
            let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
            let rows = stmt
                .query_map(params![limit as i64, slug, role], message_record_row)
                .map_err(sqlite_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
        }
        (Some(slug), None) => {
            let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
            let rows = stmt
                .query_map(params![limit as i64, slug], message_record_row)
                .map_err(sqlite_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
        }
        (None, Some(role)) => {
            let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
            let rows = stmt
                .query_map(params![limit as i64, role], message_record_row)
                .map_err(sqlite_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
        }
        (None, None) => {
            let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
            let rows = stmt
                .query_map(params![limit as i64], message_record_row)
                .map_err(sqlite_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?
        }
    };
    Ok((items, auto_sync))
}

pub fn read_message(
    paths: &ResolvedPaths,
    message_id: &str,
) -> Result<(MessageRecord, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let mut stmt = conn
        .prepare(
            "SELECT m.message_id, m.thread_id, t.project_slug, m.turn_id, m.role, m.kind,
                    m.timestamp, m.text, m.snippet
             FROM messages m
             JOIN threads t ON t.thread_id = m.thread_id
             WHERE m.message_id = ?1",
        )
        .map_err(sqlite_err)?;
    let record = stmt
        .query_row([message_id], message_record_row)
        .optional()
        .map_err(sqlite_err)?
        .ok_or_else(|| {
            AppError::with_details(
                ErrorCode::NotFound,
                format!("message '{message_id}' was not found"),
                serde_json::json!({ "message_id": message_id }),
            )
        })?;
    Ok((record, auto_sync))
}

pub fn read_events(
    paths: &ResolvedPaths,
    thread_id: &str,
    limit: usize,
) -> Result<(Vec<EventRecord>, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let mut stmt = conn
        .prepare(
            "SELECT event_id, thread_id, ordinal, timestamp, record_type, payload_type,
                    file_path, byte_start, byte_len
             FROM events
             WHERE thread_id = ?1
             ORDER BY ordinal
             LIMIT ?2",
        )
        .map_err(sqlite_err)?;
    let rows = stmt
        .query_map(params![thread_id, limit as i64], |row| {
            Ok(IndexedEvent {
                event_id: row.get(0)?,
                thread_id: row.get(1)?,
                ordinal: row.get(2)?,
                timestamp: row.get(3)?,
                record_type: row.get(4)?,
                payload_type: row.get(5)?,
                file_path: Utf8PathBuf::from(row.get::<_, String>(6)?),
                byte_start: row.get(7)?,
                byte_len: row.get(8)?,
            })
        })
        .map_err(sqlite_err)?;
    let indexed = rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?;
    if indexed.is_empty() {
        return Err(AppError::with_details(
            ErrorCode::NotFound,
            format!("thread '{thread_id}' was not found"),
            serde_json::json!({ "thread_id": thread_id }),
        ));
    }
    let file_path = indexed[0].file_path.clone();
    let mut file = File::open(&file_path)
        .map_err(|error| io_error(&format!("failed to open {file_path}"), error))?;
    let mut records = Vec::with_capacity(indexed.len());
    for event in indexed {
        let payload = read_payload(&mut file, &event)?;
        records.push(EventRecord {
            event_id: event.event_id,
            thread_id: event.thread_id,
            ordinal: event.ordinal,
            timestamp: event.timestamp,
            record_type: event.record_type,
            payload_type: event.payload_type,
            payload,
        });
    }
    Ok((records, auto_sync))
}

pub fn stats(paths: &ResolvedPaths) -> Result<(StatsRecord, bool), AppError> {
    let auto_sync = ensure_fresh(paths)?;
    let conn = open_connection(paths, false)?;
    let last_sync_at: Option<String> = conn
        .query_row(
            "SELECT value FROM state WHERE key = 'last_sync_at'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_err)?;
    let source_file_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    let project_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    let thread_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    let message_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    let event_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .map_err(sqlite_err)?;

    Ok((
        StatsRecord {
            index_path: paths.index_path.to_string(),
            last_sync_at,
            source_file_count,
            project_count,
            thread_count,
            message_count,
            event_count,
            sessions_root: paths.sessions_root.to_string(),
            archived_root: paths.archived_root.to_string(),
        },
        auto_sync,
    ))
}

fn needs_sync(paths: &ResolvedPaths, inventory: &ArchiveInventory) -> Result<bool, AppError> {
    if !paths.index_path.exists() {
        return Ok(true);
    }

    let conn = open_connection(paths, false)?;
    let db_files = load_file_state(&conn)?;
    if db_files.len() != inventory.files.len() {
        return Ok(true);
    }
    let current_paths = inventory
        .files
        .iter()
        .map(|item| item.path.as_str())
        .collect::<BTreeSet<_>>();
    let indexed_paths = db_files.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if current_paths != indexed_paths {
        return Ok(true);
    }
    for item in &inventory.files {
        let Some(state) = db_files.get(item.path.as_str()) else {
            return Ok(true);
        };
        if state.thread_id != item.thread_id
            || state.size != item.size
            || state.mtime_ns != item.mtime_ns
        {
            return Ok(true);
        }
    }

    let indexed_session_index_mtime_ns: Option<i64> = conn
        .query_row(
            "SELECT value FROM state WHERE key = 'session_index_mtime_ns'",
            [],
            |row| {
                let value: String = row.get(0)?;
                Ok(value.parse::<i64>().ok())
            },
        )
        .optional()
        .map_err(sqlite_err)?
        .flatten();
    Ok(indexed_session_index_mtime_ns != inventory.session_index_mtime_ns)
}

fn sync_with_inventory(
    paths: &ResolvedPaths,
    inventory: ArchiveInventory,
    rebuild: bool,
) -> Result<SyncSummary, AppError> {
    paths.ensure_index_dir()?;
    let mut conn = open_connection(paths, true)?;
    if rebuild {
        reset_schema(&conn)?;
    }
    init_schema(&conn)?;

    let existing = load_file_state(&conn)?;
    let current_paths = inventory
        .files
        .iter()
        .map(|item| item.path.to_string())
        .collect::<BTreeSet<_>>();
    let mut removed = Vec::new();
    for path in existing.keys() {
        if !current_paths.contains(path) {
            removed.push(path.clone());
        }
    }
    let mut updated = Vec::new();
    for item in &inventory.files {
        let changed = existing
            .get(item.path.as_str())
            .map(|state| {
                state.thread_id != item.thread_id
                    || state.size != item.size
                    || state.mtime_ns != item.mtime_ns
            })
            .unwrap_or(true);
        if changed {
            updated.push(item.clone());
        }
    }

    let transaction = conn.transaction().map_err(sqlite_err)?;
    for path in &removed {
        delete_file_records(&transaction, path)?;
    }
    for item in &updated {
        delete_thread_records(&transaction, &item.thread_id)?;
        let parsed = parse_thread(item, inventory.titles.get(&item.thread_id))?;
        insert_parsed_thread(&transaction, item, &parsed)?;
    }
    apply_title_updates(&transaction, &inventory.titles)?;
    refresh_projects(&transaction)?;
    update_state(&transaction, &inventory)?;
    transaction.commit().map_err(sqlite_err)?;

    let project_count = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    let thread_count = conn
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    let message_count = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    let event_count = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .map_err(sqlite_err)?;

    Ok(SyncSummary {
        discovered_files: inventory.files.len(),
        updated_files: updated.len(),
        removed_files: removed.len(),
        project_count,
        thread_count,
        message_count,
        event_count,
        rebuilt: rebuild,
    })
}

fn schema_requires_rebuild(paths: &ResolvedPaths) -> Result<bool, AppError> {
    if !paths.index_path.exists() {
        return Ok(false);
    }
    let conn = open_connection(paths, false)?;
    index_schema_mismatch(&conn)
}

fn index_schema_mismatch(conn: &Connection) -> Result<bool, AppError> {
    let has_state: bool = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'state'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_err)?;
    if !has_state {
        return Ok(true);
    }
    let schema_version: Option<String> = conn
        .query_row(
            "SELECT value FROM state WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_err)?;
    Ok(schema_version.as_deref() != Some(SCHEMA_VERSION))
}

fn open_connection(paths: &ResolvedPaths, create_dirs: bool) -> Result<Connection, AppError> {
    if create_dirs {
        paths.ensure_index_dir()?;
    }
    let conn = Connection::open(&paths.index_path).map_err(sqlite_err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(sqlite_err)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        CREATE TABLE IF NOT EXISTS state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            archived INTEGER NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS projects (
            project_slug TEXT PRIMARY KEY,
            project_cwd TEXT,
            thread_count INTEGER NOT NULL,
            last_seen_at TEXT
        );
        CREATE TABLE IF NOT EXISTS threads (
            thread_id TEXT PRIMARY KEY,
            project_slug TEXT,
            project_cwd TEXT,
            path TEXT NOT NULL,
            archived INTEGER NOT NULL,
            default_scope INTEGER NOT NULL,
            title TEXT,
            updated_at TEXT,
            started_at TEXT,
            source_kind TEXT NOT NULL,
            cwd TEXT,
            cli_version TEXT,
            has_subagents INTEGER NOT NULL,
            message_count INTEGER NOT NULL,
            event_count INTEGER NOT NULL,
            search_text TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS messages (
            message_id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            turn_id TEXT,
            role TEXT NOT NULL,
            kind TEXT NOT NULL,
            timestamp TEXT,
            text TEXT NOT NULL,
            snippet TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS events (
            event_id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            timestamp TEXT,
            record_type TEXT NOT NULL,
            payload_type TEXT,
            file_path TEXT NOT NULL,
            byte_start INTEGER NOT NULL,
            byte_len INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_messages_thread_id ON messages(thread_id, ordinal);
        CREATE INDEX IF NOT EXISTS idx_events_thread_id ON events(thread_id, ordinal);
        CREATE INDEX IF NOT EXISTS idx_threads_project ON threads(project_slug, updated_at DESC);
        CREATE VIRTUAL TABLE IF NOT EXISTS thread_fts USING fts5(
            thread_id UNINDEXED,
            title,
            search_text
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
            message_id UNINDEXED,
            thread_id UNINDEXED,
            text
        );
        ",
    )
    .map_err(sqlite_err)
}

fn reset_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        DROP TABLE IF EXISTS state;
        DROP TABLE IF EXISTS files;
        DROP TABLE IF EXISTS projects;
        DROP TABLE IF EXISTS threads;
        DROP TABLE IF EXISTS messages;
        DROP TABLE IF EXISTS events;
        DROP TABLE IF EXISTS thread_fts;
        DROP TABLE IF EXISTS message_fts;
        ",
    )
    .map_err(sqlite_err)
}

fn load_file_state(conn: &Connection) -> Result<BTreeMap<String, FileState>, AppError> {
    let mut stmt = conn
        .prepare("SELECT path, thread_id, size, mtime_ns FROM files")
        .map_err(sqlite_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(FileState {
                path: row.get(0)?,
                thread_id: row.get(1)?,
                size: row.get(2)?,
                mtime_ns: row.get(3)?,
            })
        })
        .map_err(sqlite_err)?;
    let states = rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?;
    Ok(states
        .into_iter()
        .map(|state| (state.path.clone(), state))
        .collect())
}

fn delete_file_records(tx: &Transaction<'_>, path: &str) -> Result<(), AppError> {
    let thread_id: Option<String> = tx
        .query_row(
            "SELECT thread_id FROM files WHERE path = ?1",
            [path],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_err)?;
    if let Some(thread_id) = thread_id {
        delete_thread_records(tx, &thread_id)?;
    }
    tx.execute("DELETE FROM files WHERE path = ?1", [path])
        .map_err(sqlite_err)?;
    Ok(())
}

fn delete_thread_records(tx: &Transaction<'_>, thread_id: &str) -> Result<(), AppError> {
    tx.execute("DELETE FROM message_fts WHERE thread_id = ?1", [thread_id])
        .map_err(sqlite_err)?;
    tx.execute("DELETE FROM thread_fts WHERE thread_id = ?1", [thread_id])
        .map_err(sqlite_err)?;
    tx.execute("DELETE FROM messages WHERE thread_id = ?1", [thread_id])
        .map_err(sqlite_err)?;
    tx.execute("DELETE FROM events WHERE thread_id = ?1", [thread_id])
        .map_err(sqlite_err)?;
    tx.execute("DELETE FROM threads WHERE thread_id = ?1", [thread_id])
        .map_err(sqlite_err)?;
    tx.execute("DELETE FROM files WHERE thread_id = ?1", [thread_id])
        .map_err(sqlite_err)?;
    Ok(())
}

fn insert_parsed_thread(
    tx: &Transaction<'_>,
    file: &crate::archive::DiscoveredFile,
    parsed: &ParsedThread,
) -> Result<(), AppError> {
    let thread = &parsed.thread;
    tx.execute(
        "INSERT INTO files(path, thread_id, archived, size, mtime_ns)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![
            file.path.as_str(),
            file.thread_id,
            bool_to_i64(file.archived),
            file.size,
            file.mtime_ns
        ],
    )
    .map_err(sqlite_err)?;
    tx.execute(
        "INSERT INTO threads(
            thread_id, project_slug, project_cwd, path, archived, default_scope, title,
            updated_at, started_at, source_kind, cwd, cli_version, has_subagents,
            message_count, event_count, search_text
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            thread.thread_id,
            thread.project_slug,
            thread.project_cwd,
            thread.path.as_str(),
            bool_to_i64(thread.archived),
            bool_to_i64(thread.default_scope),
            thread.title,
            thread.updated_at,
            thread.started_at,
            thread.source_kind,
            thread.cwd,
            thread.cli_version,
            bool_to_i64(thread.has_subagents),
            thread.message_count,
            thread.event_count,
            thread.search_text
        ],
    )
    .map_err(sqlite_err)?;
    tx.execute(
        "INSERT INTO thread_fts(thread_id, title, search_text) VALUES(?1, ?2, ?3)",
        params![thread.thread_id, thread.title, thread.search_text],
    )
    .map_err(sqlite_err)?;
    insert_messages(tx, &parsed.messages)?;
    insert_events(tx, &parsed.events)?;
    Ok(())
}

fn insert_messages(tx: &Transaction<'_>, messages: &[IndexedMessage]) -> Result<(), AppError> {
    let mut insert = tx
        .prepare(
            "INSERT INTO messages(
                message_id, thread_id, ordinal, turn_id, role, kind, timestamp, text, snippet
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .map_err(sqlite_err)?;
    let mut insert_fts = tx
        .prepare("INSERT INTO message_fts(message_id, thread_id, text) VALUES(?1, ?2, ?3)")
        .map_err(sqlite_err)?;
    for message in messages {
        insert
            .execute(params![
                message.message_id,
                message.thread_id,
                message.ordinal,
                message.turn_id,
                message.role,
                message.kind,
                message.timestamp,
                message.text,
                message.snippet
            ])
            .map_err(sqlite_err)?;
        insert_fts
            .execute(params![message.message_id, message.thread_id, message.text])
            .map_err(sqlite_err)?;
    }
    Ok(())
}

fn insert_events(tx: &Transaction<'_>, events: &[IndexedEvent]) -> Result<(), AppError> {
    let mut insert = tx
        .prepare(
            "INSERT INTO events(
                event_id, thread_id, ordinal, timestamp, record_type, payload_type,
                file_path, byte_start, byte_len
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .map_err(sqlite_err)?;
    for event in events {
        insert
            .execute(params![
                event.event_id,
                event.thread_id,
                event.ordinal,
                event.timestamp,
                event.record_type,
                event.payload_type,
                event.file_path.as_str(),
                event.byte_start,
                event.byte_len
            ])
            .map_err(sqlite_err)?;
    }
    Ok(())
}

fn refresh_projects(tx: &Transaction<'_>) -> Result<(), AppError> {
    tx.execute("DELETE FROM projects", []).map_err(sqlite_err)?;
    tx.execute(
        "INSERT INTO projects(project_slug, project_cwd, thread_count, last_seen_at)
         SELECT project_slug,
                MAX(project_cwd),
                COUNT(*) FILTER (WHERE default_scope = 1),
                MAX(updated_at) FILTER (WHERE default_scope = 1)
         FROM threads
         WHERE project_slug IS NOT NULL
         GROUP BY project_slug
         HAVING COUNT(*) FILTER (WHERE default_scope = 1) > 0",
        [],
    )
    .map_err(sqlite_err)?;
    Ok(())
}

fn apply_title_updates(
    tx: &Transaction<'_>,
    titles: &BTreeMap<String, crate::archive::TitleInfo>,
) -> Result<(), AppError> {
    let mut update = tx
        .prepare("UPDATE threads SET title = ?2, updated_at = COALESCE(?3, updated_at) WHERE thread_id = ?1")
        .map_err(sqlite_err)?;
    let mut update_fts = tx
        .prepare("UPDATE thread_fts SET title = ?2 WHERE thread_id = ?1")
        .map_err(sqlite_err)?;
    for (thread_id, title_info) in titles {
        update
            .execute(params![
                thread_id,
                title_info.thread_name,
                title_info.updated_at
            ])
            .map_err(sqlite_err)?;
        update_fts
            .execute(params![thread_id, title_info.thread_name])
            .map_err(sqlite_err)?;
    }
    Ok(())
}

fn update_state(tx: &Transaction<'_>, inventory: &ArchiveInventory) -> Result<(), AppError> {
    let now = crate::envelope::now_rfc3339();
    tx.execute(
        "INSERT INTO state(key, value) VALUES('last_sync_at', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [now],
    )
    .map_err(sqlite_err)?;
    tx.execute(
        "INSERT INTO state(key, value) VALUES('session_index_mtime_ns', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [inventory
            .session_index_mtime_ns
            .map(|value| value.to_string())
            .unwrap_or_default()],
    )
    .map_err(sqlite_err)?;
    tx.execute(
        "INSERT INTO state(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [SCHEMA_VERSION],
    )
    .map_err(sqlite_err)?;
    Ok(())
}

fn exact_title_matches(conn: &Connection, query: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT thread_id FROM threads
             WHERE default_scope = 1
               AND lower(COALESCE(title, '')) = lower(?1)
             ORDER BY COALESCE(updated_at, '') DESC, thread_id",
        )
        .map_err(sqlite_err)?;
    let rows = stmt
        .query_map([query], |row| row.get(0))
        .map_err(sqlite_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)
}

fn thread_hits_for_ids(
    conn: &Connection,
    thread_ids: &[String],
) -> Result<Vec<ThreadSearchHit>, AppError> {
    let mut hits = Vec::with_capacity(thread_ids.len());
    let mut stmt = conn
        .prepare(
            "SELECT thread_id, project_slug, title, updated_at, started_at, source_kind, cwd
             FROM threads
             WHERE thread_id = ?1",
        )
        .map_err(sqlite_err)?;
    for thread_id in thread_ids {
        let hit = stmt
            .query_row([thread_id], |row| {
                let title: Option<String> = row.get(2)?;
                Ok(ThreadSearchHit {
                    thread_id: row.get(0)?,
                    project_slug: row.get(1)?,
                    title: title.clone(),
                    updated_at: row.get(3)?,
                    started_at: row.get(4)?,
                    source_kind: row.get(5)?,
                    cwd: row.get(6)?,
                    snippet: title.unwrap_or_default(),
                })
            })
            .optional()
            .map_err(sqlite_err)?;
        if let Some(hit) = hit {
            hits.push(hit);
        }
    }
    Ok(hits)
}

fn search_threads_inner(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<ThreadSearchHit>, AppError> {
    let query = fts_query(query);
    let mut stmt = conn
        .prepare(
            "SELECT t.thread_id, t.project_slug, t.title, t.updated_at, t.started_at,
                    t.source_kind, t.cwd,
                    snippet(thread_fts, 1, '', '', ' ... ', 14) AS snippet
             FROM thread_fts
             JOIN threads t USING(thread_id)
             WHERE thread_fts MATCH ?1 AND t.default_scope = 1
             ORDER BY bm25(thread_fts), COALESCE(t.updated_at, '') DESC, t.thread_id
             LIMIT ?2",
        )
        .map_err(sqlite_err)?;
    let rows = stmt
        .query_map(params![query, limit as i64], thread_search_hit_row)
        .map_err(sqlite_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)
}

fn thread_search_hit_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadSearchHit> {
    Ok(ThreadSearchHit {
        thread_id: row.get(0)?,
        project_slug: row.get(1)?,
        title: row.get(2)?,
        updated_at: row.get(3)?,
        started_at: row.get(4)?,
        source_kind: row.get(5)?,
        cwd: row.get(6)?,
        snippet: row.get(7)?,
    })
}

fn thread_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadRecord> {
    Ok(ThreadRecord {
        thread_id: row.get(0)?,
        project_slug: row.get(1)?,
        project_cwd: row.get(2)?,
        title: row.get(3)?,
        updated_at: row.get(4)?,
        started_at: row.get(5)?,
        source_kind: row.get(6)?,
        cwd: row.get(7)?,
        cli_version: row.get(8)?,
        has_subagents: row.get::<_, i64>(9)? != 0,
        message_count: row.get(10)?,
        event_count: row.get(11)?,
        archived: row.get::<_, i64>(12)? != 0,
        default_scope: row.get::<_, i64>(13)? != 0,
        path: row.get(14)?,
    })
}

fn message_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        message_id: row.get(0)?,
        thread_id: row.get(1)?,
        project_slug: row.get(2)?,
        turn_id: row.get(3)?,
        role: row.get(4)?,
        kind: row.get(5)?,
        timestamp: row.get(6)?,
        text: row.get(7)?,
        snippet: row.get(8)?,
    })
}

fn read_thread_from_conn(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<ThreadRecord>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT thread_id, project_slug, project_cwd, title, updated_at, started_at,
                    source_kind, cwd, cli_version, has_subagents, message_count, event_count,
                    archived, default_scope, path
             FROM threads WHERE thread_id = ?1",
        )
        .map_err(sqlite_err)?;
    stmt.query_row([thread_id], thread_record_row)
        .optional()
        .map_err(sqlite_err)
}

fn resolve_project_filter(
    conn: &Connection,
    project: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(query) = project else {
        return Ok(None);
    };
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut by_slug = conn
        .prepare("SELECT project_slug FROM projects WHERE project_slug = ?1")
        .map_err(sqlite_err)?;
    if let Some(slug) = by_slug
        .query_row([trimmed], |row| row.get::<_, String>(0))
        .optional()
        .map_err(sqlite_err)?
    {
        return Ok(Some(slug));
    }

    let mut by_cwd = conn
        .prepare("SELECT project_slug FROM projects WHERE project_cwd = ?1")
        .map_err(sqlite_err)?;
    if let Some(slug) = by_cwd
        .query_row([trimmed], |row| row.get::<_, String>(0))
        .optional()
        .map_err(sqlite_err)?
    {
        return Ok(Some(slug));
    }

    let mut by_substring = conn
        .prepare(
            "SELECT project_slug FROM projects
             WHERE project_slug LIKE ?1 OR project_cwd LIKE ?1
             ORDER BY thread_count DESC, project_slug
             LIMIT 5",
        )
        .map_err(sqlite_err)?;
    let pattern = format!("%{trimmed}%");
    let rows = by_substring
        .query_map([pattern], |row| row.get::<_, String>(0))
        .map_err(sqlite_err)?;
    let mut matches = rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)?;
    if matches.is_empty() {
        return Err(AppError::with_details(
            ErrorCode::NotFound,
            format!("no project matched '{trimmed}'"),
            serde_json::json!({ "project": trimmed }),
        ));
    }
    if matches.len() > 1 {
        return Err(AppError::with_details(
            ErrorCode::Ambiguous,
            format!("multiple projects matched '{trimmed}'"),
            serde_json::json!({ "candidates": matches }),
        ));
    }
    Ok(Some(matches.remove(0)))
}

fn read_payload(file: &mut File, event: &IndexedEvent) -> Result<Value, AppError> {
    file.seek(SeekFrom::Start(event.byte_start as u64))
        .map_err(|error| io_error("failed to seek within event source file", error))?;
    let mut buffer = vec![0_u8; event.byte_len as usize];
    file.read_exact(&mut buffer)
        .map_err(|error| io_error("failed to read event source bytes", error))?;
    let line = String::from_utf8(buffer)
        .map_err(|error| internal(format!("event bytes were not valid UTF-8: {error}")))?;
    let value: Value = serde_json::from_str(line.trim_end())
        .map_err(|error| internal(format!("failed to re-parse indexed event: {error}")))?;
    Ok(value
        .get("payload")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default())))
}

fn sqlite_err(error: rusqlite::Error) -> AppError {
    AppError::with_details(
        ErrorCode::InternalError,
        format!("sqlite error: {error}"),
        serde_json::json!({ "sqlite_error": error.to_string() }),
    )
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn fts_query(input: &str) -> String {
    let terms = input
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        format!("\"{}\"", input.replace('"', "\"\""))
    } else {
        terms.join(" ")
    }
}
