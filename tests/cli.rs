use assert_cmd::Command;
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const THREAD_ONE: &str = "11111111-1111-4111-8111-111111111111";
const THREAD_TWO: &str = "22222222-2222-4222-8222-222222222222";
const SUBAGENT_THREAD: &str = "33333333-3333-4333-8333-333333333333";
const THREAD_EARLY: &str = "12121212-1212-4121-8121-121212121212";
const THREAD_LATE: &str = "13131313-1313-4131-8131-131313131313";
const THREAD_UNTIMESTAMPED: &str = "14141414-1414-4141-8141-141414141414";

const PROJECT_CODEX_CWD: &str = "/workspace/codex-threads";
const PROJECT_ARCHIVE_CWD: &str = "/workspace/archive";

const PROJECT_CODEX_SLUG: &str = "~2Fworkspace~2Fcodex~2Dthreads";
const PROJECT_IDEAS_SLUG: &str = "~2Fworkspace~2Fideas";
const PROJECT_ARCHIVE_SLUG: &str = "~2Fworkspace~2Farchive";

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

fn write_session(temp: &TempDir, relative_path: &str, contents: &str) {
    let path = temp.path().join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create session parent");
    }
    fs::write(path, contents).expect("write session");
}

fn write_session_jsonl(temp: &TempDir, relative_path: &str, records: &[Value]) {
    let contents = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    write_session(temp, relative_path, &format!("{contents}\n"));
}

fn append_session_index(temp: &TempDir, line: &str) {
    let path = temp.path().join("session_index.jsonl");
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open session index");
    writeln!(file, "{line}").expect("append session index");
}

fn add_same_project_ordering_threads(temp: &TempDir) {
    write_session(
        temp,
        "sessions/2026/04/11/rollout-2026-04-11T09-00-00-12121212-1212-4121-8121-121212121212.jsonl",
        "{\"timestamp\":\"2026-04-11T09:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"12121212-1212-4121-8121-121212121212\",\"timestamp\":\"2026-04-11T09:00:00Z\",\"cwd\":\"/workspace/codex-threads\",\"cli_version\":\"0.120.0\",\"source\":\"cli\"}}\n{\"timestamp\":\"2026-04-11T09:00:01Z\",\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-early\",\"cwd\":\"/workspace/codex-threads\",\"model\":\"gpt-5.4\",\"summary\":\"earliest\"}}\n{\"timestamp\":\"2026-04-11T09:00:02Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"turn_id\":\"turn-early\",\"message\":\"first project question from the codex workspace\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n{\"timestamp\":\"2026-04-11T09:00:03Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Earliest reply for the codex workspace.\"}]}}\n",
    );
    write_session(
        temp,
        "sessions/2026/04/11/rollout-2026-04-11T12-30-00-13131313-1313-4131-8131-131313131313.jsonl",
        "{\"timestamp\":\"2026-04-11T12:30:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"13131313-1313-4131-8131-131313131313\",\"timestamp\":\"2026-04-11T12:30:00Z\",\"cwd\":\"/workspace/codex-threads\",\"cli_version\":\"0.120.0\",\"source\":\"cli\"}}\n{\"timestamp\":\"2026-04-11T12:30:01Z\",\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-late\",\"cwd\":\"/workspace/codex-threads\",\"model\":\"gpt-5.4\",\"summary\":\"latest\"}}\n{\"timestamp\":\"2026-04-11T12:30:02Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"turn_id\":\"turn-late\",\"message\":\"last project question from the codex workspace\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n{\"timestamp\":\"2026-04-11T12:30:03Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Latest reply for the codex workspace.\"}]}}\n",
    );
    append_session_index(
        temp,
        "{\"id\":\"12121212-1212-4121-8121-121212121212\",\"thread_name\":\"Earliest codex thread\",\"updated_at\":\"2026-04-11T09:00:04Z\"}",
    );
    append_session_index(
        temp,
        "{\"id\":\"13131313-1313-4131-8131-131313131313\",\"thread_name\":\"Latest codex thread\",\"updated_at\":\"2026-04-11T12:30:04Z\"}",
    );
}

fn add_same_project_untimestamped_thread(temp: &TempDir) {
    write_session(
        temp,
        "sessions/2026/04/11/rollout-2026-04-11T13-30-00-14141414-1414-4141-8141-141414141414.jsonl",
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"14141414-1414-4141-8141-141414141414\",\"cwd\":\"/workspace/codex-threads\",\"cli_version\":\"0.120.0\",\"source\":\"cli\"}}\n{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-untimed\",\"cwd\":\"/workspace/codex-threads\",\"model\":\"gpt-5.4\",\"summary\":\"untimestamped\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"turn_id\":\"turn-untimed\",\"message\":\"untimestamped project question from the codex workspace\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Untimestamped reply for the codex workspace.\"}]}}\n",
    );
}

#[test]
fn sync_indexes_fixture_archives() {
    let temp = copied_fixture_home();
    let (status, json, _stderr) = run_json(&temp, &["--json", "sync"]);
    assert_eq!(status, 0);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["discovered_files"], 4);
    assert_eq!(json["data"]["project_count"], 3);
    assert_eq!(json["data"]["thread_count"], 4);
    assert_eq!(json["data"]["message_count"], 9);
    assert_eq!(json["data"]["event_count"], 17);
}

#[test]
fn current_rollout_schema_indexes_user_turns_and_top_level_events() {
    let temp = copied_fixture_home();
    let thread_id = "99999999-9999-4999-8999-999999999998";
    write_session_jsonl(
        &temp,
        "sessions/2026/09/29/rollout-2026-09-29T10-00-00-99999999-9999-4999-8999-999999999998.jsonl",
        &[
            serde_json::json!({"timestamp":"2026-09-29T10:00:00Z","type":"session_meta","payload":{"id":thread_id,"timestamp":"2026-09-29T10:00:00Z","cwd":"/workspace/current","source":"exec","thread_source":"user"}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-current"}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:02Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"private instruction marker"}]}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:03Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"current schema unique phrase"},{"type":"input_image","image_url":"data:image/png;base64,AAAA"},{"type":"input_text","text":"second part"}]}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:04Z","type":"event_msg","payload":{"type":"user_message","message":"current schema unique phrase\nsecond part"}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:05Z","type":"world_state","full":true,"state":{"status":"ready"}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:06Z","type":"turn_context","payload":{"turn_id":"turn-current","cwd":"/workspace/current"}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:07Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Current reply."}]}}),
            serde_json::json!({"timestamp":"2026-09-29T10:00:08Z","type":"token_usage_record","thread_id":thread_id,"usage":{"input_tokens":2}}),
        ],
    );

    let (status, thread_json, _stderr) = run_json(&temp, &["--json", "threads", "read", thread_id]);
    assert_eq!(status, 0);
    assert_eq!(thread_json["data"]["thread"]["message_count"], 2);
    assert_eq!(
        thread_json["data"]["thread"]["title"],
        "current schema unique phrase second part"
    );

    let (status, messages_json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "list",
            "--project",
            "/workspace/current",
            "--role",
            "user",
        ],
    );
    assert_eq!(status, 0);
    let messages = messages_json["data"]["items"].as_array().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["text"],
        "current schema unique phrase\nsecond part"
    );
    assert_eq!(messages[0]["kind"], "user_message");

    let (status, search_json, _stderr) = run_json(
        &temp,
        &["--json", "messages", "search", "schema unique phrase"],
    );
    assert_eq!(status, 0);
    assert_eq!(search_json["data"]["items"][0]["thread_id"], thread_id);

    let (status, events_json, _stderr) = run_json(
        &temp,
        &["--json", "events", "read", thread_id, "--limit", "20"],
    );
    assert_eq!(status, 0);
    let events = events_json["data"]["items"].as_array().expect("events");
    assert_eq!(events.len(), 9);
    let world_state = events
        .iter()
        .find(|event| event["record_type"] == "world_state")
        .expect("world state event");
    assert_eq!(world_state["payload"]["state"]["status"], "ready");
    let usage = events
        .iter()
        .find(|event| event["record_type"] == "token_usage_record")
        .expect("usage event");
    assert_eq!(usage["payload"]["usage"]["input_tokens"], 2);
}

#[test]
fn current_subagent_source_stays_out_of_default_search() {
    let temp = copied_fixture_home();
    let thread_id = "99999999-9999-4999-8999-999999999997";
    write_session_jsonl(
        &temp,
        "sessions/2026/09/29/rollout-2026-09-29T11-00-00-99999999-9999-4999-8999-999999999997.jsonl",
        &[
            serde_json::json!({"timestamp":"2026-09-29T11:00:00Z","type":"session_meta","payload":{"id":thread_id,"cwd":"/workspace/current","source":{"subagent":{"thread_spawn":{"parent_thread_id":THREAD_ONE,"depth":1}}},"thread_source":"subagent"}}),
            serde_json::json!({"timestamp":"2026-09-29T11:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"nested subagent unique phrase"}]}}),
        ],
    );

    let (status, thread_json, _stderr) = run_json(&temp, &["--json", "threads", "read", thread_id]);
    assert_eq!(status, 0);
    assert_eq!(
        thread_json["data"]["thread"]["source_kind"],
        "subagent:thread_spawn"
    );
    assert_eq!(thread_json["data"]["thread"]["default_scope"], false);

    let (status, search_json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "nested subagent unique phrase",
        ],
    );
    assert_eq!(status, 0);
    assert!(
        search_json["data"]["items"]
            .as_array()
            .expect("items")
            .is_empty()
    );
}

#[test]
fn thread_source_fallback_hides_known_subagents_and_keeps_feature_threads() {
    let temp = copied_fixture_home();
    let feature_id = "99999999-9999-4999-8999-999999999996";
    let subagent_id = "99999999-9999-4999-8999-999999999995";
    write_session_jsonl(
        &temp,
        "sessions/2026/09/29/rollout-2026-09-29T12-00-00-99999999-9999-4999-8999-999999999996.jsonl",
        &[
            serde_json::json!({"type":"session_meta","payload":{"id":feature_id,"cwd":"/workspace/current","thread_source":"feature:import"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"feature source query marker"}]}}),
        ],
    );
    write_session_jsonl(
        &temp,
        "sessions/2026/09/29/rollout-2026-09-29T12-01-00-99999999-9999-4999-8999-999999999995.jsonl",
        &[
            serde_json::json!({"type":"session_meta","payload":{"id":subagent_id,"cwd":"/workspace/current","thread_source":"subagent"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"fallback subagent query marker"}]}}),
        ],
    );

    let (status, search_json, _stderr) = run_json(
        &temp,
        &["--json", "threads", "search", "source query marker"],
    );
    assert_eq!(status, 0);
    let hits = search_json["data"]["items"].as_array().expect("hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["thread_id"], feature_id);

    let (status, thread_json, _stderr) =
        run_json(&temp, &["--json", "threads", "read", subagent_id]);
    assert_eq!(status, 0);
    assert_eq!(thread_json["data"]["thread"]["default_scope"], false);
    assert_eq!(thread_json["data"]["thread"]["has_subagents"], true);
}

#[test]
fn projects_list_returns_known_projects_in_recency_order() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);
    let (status, json, _stderr) = run_json(&temp, &["--json", "projects", "list"]);
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["project_slug"], PROJECT_IDEAS_SLUG);
    assert_eq!(items[1]["project_slug"], PROJECT_CODEX_SLUG);
    assert_eq!(items[2]["project_slug"], PROJECT_ARCHIVE_SLUG);
    assert_eq!(items[2]["project_cwd"], PROJECT_ARCHIVE_CWD);
    assert_eq!(items[2]["thread_count"], 1);
}

#[test]
fn threads_list_project_filter_orders_chronologically() {
    let temp = copied_fixture_home();
    add_same_project_ordering_threads(&temp);
    add_same_project_untimestamped_thread(&temp);
    let _ = run_json(&temp, &["--json", "sync"]);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "list",
            "--project",
            PROJECT_CODEX_SLUG,
            "--order",
            "asc",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    assert_eq!(json["data"]["order"], "asc");
    let items = json["data"]["items"].as_array().expect("items array");
    let thread_ids = items
        .iter()
        .map(|item| item["thread_id"].as_str().expect("thread id"))
        .collect::<Vec<_>>();
    assert_eq!(
        thread_ids,
        vec![THREAD_EARLY, THREAD_ONE, THREAD_LATE, THREAD_UNTIMESTAMPED]
    );

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "list",
            "--project",
            PROJECT_CODEX_SLUG,
            "--order",
            "desc",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    assert_eq!(json["data"]["order"], "desc");
    let items = json["data"]["items"].as_array().expect("items array");
    let thread_ids = items
        .iter()
        .map(|item| item["thread_id"].as_str().expect("thread id"))
        .collect::<Vec<_>>();
    assert_eq!(
        thread_ids,
        vec![THREAD_LATE, THREAD_ONE, THREAD_EARLY, THREAD_UNTIMESTAMPED]
    );
    assert!(
        items
            .iter()
            .all(|item| item["thread_id"] != SUBAGENT_THREAD && item["default_scope"] == true)
    );
}

#[test]
fn messages_list_project_filter_supports_first_and_last_user_queries() {
    let temp = copied_fixture_home();
    add_same_project_ordering_threads(&temp);
    add_same_project_untimestamped_thread(&temp);
    let _ = run_json(&temp, &["--json", "sync"]);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "list",
            "--project",
            PROJECT_CODEX_CWD,
            "--role",
            "user",
            "--order",
            "asc",
            "--limit",
            "1",
        ],
    );
    assert_eq!(status, 0);
    assert_eq!(json["data"]["order"], "asc");
    assert_eq!(json["data"]["role"], "user");
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], THREAD_EARLY);
    assert_eq!(
        items[0]["text"],
        "first project question from the codex workspace"
    );

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "list",
            "--project",
            PROJECT_CODEX_CWD,
            "--role",
            "user",
            "--order",
            "desc",
            "--limit",
            "1",
        ],
    );
    assert_eq!(status, 0);
    assert_eq!(json["data"]["order"], "desc");
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], THREAD_LATE);
    assert_eq!(
        items[0]["text"],
        "last project question from the codex workspace"
    );

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "list",
            "--project",
            PROJECT_CODEX_CWD,
            "--role",
            "user",
            "--order",
            "asc",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    let thread_ids = items
        .iter()
        .map(|item| item["thread_id"].as_str().expect("thread id"))
        .collect::<Vec<_>>();
    assert_eq!(
        thread_ids,
        vec![THREAD_EARLY, THREAD_ONE, THREAD_LATE, THREAD_UNTIMESTAMPED]
    );
}

#[test]
fn messages_list_excludes_subagent_messages_and_accepts_slug_filters() {
    let temp = copied_fixture_home();
    add_same_project_ordering_threads(&temp);
    let _ = run_json(&temp, &["--json", "sync"]);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "list",
            "--project",
            PROJECT_CODEX_SLUG,
            "--order",
            "asc",
            "--limit",
            "20",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert!(!items.is_empty());
    assert!(
        items
            .iter()
            .all(|item| item["project_slug"] == PROJECT_CODEX_SLUG)
    );
    assert!(
        items
            .iter()
            .all(|item| item["thread_id"] != SUBAGENT_THREAD)
    );
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
    assert_eq!(items[0]["project_slug"], PROJECT_CODEX_SLUG);
}

#[test]
fn threads_search_project_filter_accepts_slug_and_full_cwd() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "tweet idea",
            "--project",
            PROJECT_IDEAS_SLUG,
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], THREAD_TWO);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "build a CLI",
            "--project",
            PROJECT_CODEX_CWD,
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], THREAD_ONE);
}

#[test]
fn threads_search_project_filter_reports_ambiguous_and_unknown_queries() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "tweet idea",
            "--project",
            "workspace",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 6);
    let candidates = json["error"]["details"]["candidates"]
        .as_array()
        .expect("candidate array");
    assert_eq!(candidates[0], PROJECT_ARCHIVE_SLUG);
    assert_eq!(candidates[1], PROJECT_CODEX_SLUG);
    assert_eq!(candidates[2], PROJECT_IDEAS_SLUG);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "tweet idea",
            "--project",
            "does-not-exist",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 5);
    assert_eq!(json["error"]["code"], "not_found");
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
    assert_eq!(json["data"]["thread"]["project_slug"], PROJECT_CODEX_SLUG);
    assert_eq!(json["data"]["thread"]["project_cwd"], PROJECT_CODEX_CWD);
    assert_eq!(json["data"]["thread"]["cwd"], PROJECT_CODEX_CWD);
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
    assert_eq!(
        read_json["data"]["message"]["project_slug"],
        PROJECT_CODEX_SLUG
    );
    assert!(
        read_json["data"]["message"]["text"]
            .as_str()
            .expect("message text")
            .contains("inspect the archive format")
    );
}

#[test]
fn messages_search_project_filter_excludes_other_projects() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "search",
            "tweet",
            "--project",
            PROJECT_IDEAS_SLUG,
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert!(!items.is_empty());
    assert!(
        items
            .iter()
            .all(|item| item["thread_id"] == THREAD_TWO
                && item["project_slug"] == PROJECT_IDEAS_SLUG)
    );

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "messages",
            "search",
            "tweet",
            "--project",
            PROJECT_CODEX_SLUG,
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert!(items.is_empty());
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
    assert_eq!(stats_json["data"]["project_count"], 3);
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
fn session_meta_missing_cwd_falls_back_to_turn_context_project() {
    let temp = copied_fixture_home();
    let thread_id = "55555555-5555-4555-8555-555555555555";
    write_session(
        &temp,
        "sessions/2026/04/11/rollout-2026-04-11T13-00-00-55555555-5555-4555-8555-555555555555.jsonl",
        "{\"timestamp\":\"2026-04-11T13:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"55555555-5555-4555-8555-555555555555\",\"timestamp\":\"2026-04-11T13:00:00Z\",\"cli_version\":\"0.120.0\",\"source\":\"cli\"}}\n{\"timestamp\":\"2026-04-11T13:00:01Z\",\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-5\",\"cwd\":\"/workspace/fallback-project\",\"model\":\"gpt-5.4\",\"summary\":\"fallback\"}}\n{\"timestamp\":\"2026-04-11T13:00:02Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"turn_id\":\"turn-5\",\"message\":\"fallback project search term\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n",
    );
    append_session_index(
        &temp,
        "{\"id\":\"55555555-5555-4555-8555-555555555555\",\"thread_name\":\"Fallback cwd thread\",\"updated_at\":\"2026-04-11T13:00:03Z\"}",
    );

    let (status, json, _stderr) = run_json(&temp, &["--json", "threads", "read", thread_id]);
    assert_eq!(status, 0);
    let thread = &json["data"]["thread"];
    assert_eq!(thread["project_slug"], "~2Fworkspace~2Ffallback~2Dproject");
    assert_eq!(thread["project_cwd"], "/workspace/fallback-project");
    assert_eq!(thread["cwd"], "/workspace/fallback-project");

    let (status, json, _stderr) = run_json(&temp, &["--json", "projects", "list"]);
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert!(
        items
            .iter()
            .any(|item| item["project_slug"] == "~2Fworkspace~2Ffallback~2Dproject")
    );
}

#[test]
fn session_without_any_cwd_stays_searchable_but_absent_from_projects() {
    let temp = copied_fixture_home();
    let thread_id = "66666666-6666-4666-8666-666666666666";
    write_session(
        &temp,
        "sessions/2026/04/11/rollout-2026-04-11T14-00-00-66666666-6666-4666-8666-666666666666.jsonl",
        "{\"timestamp\":\"2026-04-11T14:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"66666666-6666-4666-8666-666666666666\",\"timestamp\":\"2026-04-11T14:00:00Z\",\"cli_version\":\"0.120.0\",\"source\":\"cli\"}}\n{\"timestamp\":\"2026-04-11T14:00:01Z\",\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-6\",\"model\":\"gpt-5.4\",\"summary\":\"no cwd\"}}\n{\"timestamp\":\"2026-04-11T14:00:02Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"turn_id\":\"turn-6\",\"message\":\"no cwd special search term\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n",
    );

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "no cwd special search term",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], thread_id);
    assert!(items[0]["project_slug"].is_null());

    let (status, json, _stderr) = run_json(&temp, &["--json", "threads", "read", thread_id]);
    assert_eq!(status, 0);
    let thread = &json["data"]["thread"];
    assert!(thread["project_slug"].is_null());
    assert!(thread["project_cwd"].is_null());
    assert!(thread["cwd"].is_null());

    let (status, json, _stderr) = run_json(&temp, &["--json", "projects", "list"]);
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
}

#[test]
fn distinct_cwds_with_separator_vs_hyphen_get_distinct_project_slugs() {
    let temp = copied_fixture_home();

    write_session(
        &temp,
        "sessions/2026/04/11/rollout-2026-04-11T15-00-00-77777777-7777-4777-8777-777777777777.jsonl",
        "{\"timestamp\":\"2026-04-11T15:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"77777777-7777-4777-8777-777777777777\",\"timestamp\":\"2026-04-11T15:00:00Z\",\"cwd\":\"/workspace/foo-bar\",\"cli_version\":\"0.120.0\",\"source\":\"cli\"}}\n{\"timestamp\":\"2026-04-11T15:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"turn_id\":\"turn-7\",\"message\":\"slug collision alpha\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n",
    );
    write_session(
        &temp,
        "sessions/2026/04/11/rollout-2026-04-11T16-00-00-88888888-8888-4888-8888-888888888888.jsonl",
        "{\"timestamp\":\"2026-04-11T16:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"88888888-8888-4888-8888-888888888888\",\"timestamp\":\"2026-04-11T16:00:00Z\",\"cwd\":\"/workspace/foo/bar\",\"cli_version\":\"0.120.0\",\"source\":\"cli\"}}\n{\"timestamp\":\"2026-04-11T16:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"turn_id\":\"turn-8\",\"message\":\"slug collision beta\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n",
    );

    let (status, json, _stderr) = run_json(&temp, &["--json", "projects", "list"]);
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert!(
        items
            .iter()
            .any(|item| item["project_slug"] == "~2Fworkspace~2Ffoo~2Dbar")
    );
    assert!(
        items
            .iter()
            .any(|item| item["project_slug"] == "~2Fworkspace~2Ffoo~2Fbar")
    );

    let (status, json, _stderr) = run_json(
        &temp,
        &[
            "--json",
            "threads",
            "search",
            "slug collision",
            "--project",
            "~2Fworkspace~2Ffoo~2Dbar",
            "--limit",
            "10",
        ],
    );
    assert_eq!(status, 0);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["thread_id"],
        "77777777-7777-4777-8777-777777777777"
    );
}

#[test]
fn schema_version_mismatch_triggers_auto_rebuild() {
    let temp = copied_fixture_home();
    let _ = run_json(&temp, &["--json", "sync"]);

    let index_path = temp.path().join("codex-threads/index.sqlite");
    let conn = Connection::open(index_path).expect("open sqlite");
    conn.execute(
        "UPDATE state SET value = '2' WHERE key = 'schema_version'",
        [],
    )
    .expect("set previous schema version");

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
    assert_eq!(json["meta"]["auto_sync_performed"], true);
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items[0]["thread_id"], THREAD_ONE);
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
