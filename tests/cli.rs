use assert_cmd::Command;
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const THREAD_ONE: &str = "11111111-1111-4111-8111-111111111111";
const THREAD_TWO: &str = "22222222-2222-4222-8222-222222222222";
const SUBAGENT_THREAD: &str = "33333333-3333-4333-8333-333333333333";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-home")
}

fn copied_fixture_home() -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_dir_all(&fixture_root(), temp.path());
    temp
}

fn copy_dir_all(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create fixture dir");
    for entry in fs::read_dir(from).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let file_type = entry.file_type().expect("fixture type");
        let dest = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).expect("copy fixture file");
        }
    }
}

fn bin(temp: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("codex-threads").expect("cargo bin");
    cmd.env("CODEX_HOME", temp.path());
    cmd
}

fn run_json(temp: &TempDir, args: &[&str]) -> (i32, Value, String) {
    let output = bin(temp).args(args).output().expect("command output");
    let status = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let value = serde_json::from_str::<Value>(&stdout).expect("json stdout");
    (status, value, stderr)
}

#[test]
fn sync_indexes_fixture_archives() {
    let temp = copied_fixture_home();
    let (status, json, _stderr) = run_json(&temp, &["--json", "sync"]);
    assert_eq!(status, 0);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["discovered_files"], 4);
    assert_eq!(json["data"]["thread_count"], 4);
    assert_eq!(json["data"]["message_count"], 9);
    assert_eq!(json["data"]["event_count"], 17);
}

#[test]
fn threads_search_lazy_sync_filters_out_subagents() {
    let temp = copied_fixture_home();
    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "build a CLI",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    assert_eq!(json["ok"], true);
    assert_eq!(json["meta"]["auto_sync_performed"], true);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], THREAD_ONE);
}

#[test]
fn threads_resolve_reports_ambiguity() {
    let temp = copied_fixture_home();
    let (status, json, _stderr) = run_json(&temp, &["--json", "threads", "resolve", "tweet idea"]);
    assert_eq!(status, 6);
    assert_eq!(json["error"]["code"], "ambiguous");
    let candidates = json["error"]["details"]["candidates"]
        .as_array()
        .expect("candidate array");
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate["thread_id"].is_string())
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate["title"] == "Tweet idea")
    );
}

#[test]
fn threads_read_returns_exact_thread() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);
    let (status, json, _stderr) = run_json(&temp, &["--json", "threads", "read", THREAD_ONE]);
    assert_eq!(status, 0);
    assert_eq!(json["data"]["thread"]["thread_id"], THREAD_ONE);
    assert_eq!(json["data"]["thread"]["title"], "Design codex-threads CLI");
    assert_eq!(json["data"]["thread"]["has_subagents"], true);
}

#[test]
fn messages_search_and_read_return_normalized_messages() {
    let temp = copied_fixture_home();
    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "search",
            "archive format",
            "--limit",
            "5",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    let message_id = items[0]["message_id"].as_str().expect("message id");
    assert!(message_id.starts_with(THREAD_ONE));

    let (read_status, read_json, _stderr) =
        run_json(&temp, &["--json", "messages", "read", message_id]);
    assert_eq!(read_status, 0);
    assert_eq!(read_json["data"]["message"]["role"], "assistant");
    assert!(
        read_json["data"]["message"]["text"]
            .as_str()
            .expect("message text")
            .contains("inspect the archive format")
    );
}

#[test]
fn events_read_returns_payloads_and_limit() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);
    let (status, json, _stderr) = run_json(
        &temp,
        &["--json", "events", "read", THREAD_ONE, "--limit", "3"],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["payload"]["id"], THREAD_ONE);
}

#[test]
fn index_stats_and_debug_paths_are_available() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);
    let (stats_status, stats_json, _stderr) = run_json(&temp, &["--json", "index", "stats"]);
    assert_eq!(stats_status, 0);
    assert_eq!(stats_json["meta"]["auto_sync_performed"], false);
    assert_eq!(stats_json["data"]["thread_count"], 4);

    let (debug_status, debug_json, _stderr) = run_json(&temp, &["--json", "debug", "paths"]);
    assert_eq!(debug_status, 0);
    assert_eq!(debug_json["data"]["sessions_root_exists"], true);
    assert_eq!(debug_json["data"]["index_exists"], true);
}

#[test]
fn malformed_jsonl_returns_sync_failed() {
    let temp = copied_fixture_home();
    let broken = temp
        .path()
        .join("sessions/2026/04/11/rollout-2026-04-11T13-00-00-55555555-5555-4555-8555-555555555555.jsonl");
    fs::write(
        broken,
        "{\"timestamp\":\"2026-04-11T13:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"55555555-5555-4555-8555-555555555555\",\"source\":\"cli\"}}\nnot-json\n",
    )
    .expect("write broken fixture");

    let (status, json, _stderr) = run_json(&temp, &["--json", "sync", "--rebuild"]);
    assert_eq!(status, 7);
    assert_eq!(json["error"]["code"], "sync_failed");
}

#[test]
fn malformed_session_index_is_ignored_as_enrichment_only() {
    let temp = copied_fixture_home();
    let session_index = temp.path().join("session_index.jsonl");
    fs::write(
        &session_index,
        format!(
            "{}\nnot-json\n",
            fs::read_to_string(&session_index).expect("read session index")
        ),
    )
    .expect("write malformed session index");

    let (status, json, stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "build a CLI",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    assert_eq!(json["ok"], true);
    assert!(stderr.contains("ignoring malformed session_index.jsonl line"));
}

#[test]
fn exact_reads_return_not_found_with_stable_exit_code() {
    let temp = copied_fixture_home();
    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "read",
            "99999999-9999-4999-8999-999999999999",
        ],
    );
    assert_eq!(status, 5);
    assert_eq!(json["error"]["code"], "not_found");
}

#[test]
fn search_results_do_not_include_subagent_thread() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);
    let (status, json, _stderr) = run_json(
        &temp,
        &["--json", "threads", "search", "review", "--limit", "10"],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert!(
        items
            .iter()
            .all(|item| item["thread_id"] != SUBAGENT_THREAD)
    );
}

#[test]
fn subagent_exact_reads_do_not_include_parent_turn_content() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);
    let (thread_status, thread_json, _stderr) =
        run_json(&temp, &["--json", "threads", "read", SUBAGENT_THREAD]);
    assert_eq!(thread_status, 0);
    assert_eq!(thread_json["data"]["thread"]["message_count"], 2);
    assert_eq!(thread_json["data"]["thread"]["event_count"], 4);

    let (events_status, events_json, _stderr) = run_json(
        &temp,
        &["--json", "events", "read", SUBAGENT_THREAD, "--limit", "10"],
    );
    assert_eq!(events_status, 0);
    let payloads = events_json["data"]["items"]
        .as_array()
        .expect("event array");
    assert!(payloads.iter().all(|item| {
        !item["payload"]
            .to_string()
            .contains("must not leak into subagent reads")
    }));
}

#[test]
fn explicit_sync_then_search_is_not_stale() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);
    let (status, json, _stderr) = run_json(
        &temp,
        &["--json", "threads", "search", "tweet idea", "--limit", "10"],
    );
    assert_eq!(status, 0);
    assert_eq!(json["meta"]["auto_sync_performed"], false);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|item| item["thread_id"] == THREAD_TWO));
}

#[test]
fn search_uses_existing_index_when_auto_sync_writer_is_locked() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);

    let session_index = temp.path().join("session_index.jsonl");
    let session_index_contents = fs::read_to_string(&session_index).expect("read session index");
    fs::write(&session_index, session_index_contents).expect("touch session index");

    let index_path = temp.path().join("codex-threads/index.sqlite");
    let conn = Connection::open(index_path).expect("open sqlite");
    conn.execute_batch("PRAGMA journal_mode = WAL; BEGIN IMMEDIATE;")
        .expect("lock sqlite writer");

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "build a CLI",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    assert_eq!(json["ok"], true);
    assert_eq!(json["meta"]["auto_sync_performed"], false);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], THREAD_ONE);

    conn.execute_batch("ROLLBACK;").expect("unlock sqlite");
}
