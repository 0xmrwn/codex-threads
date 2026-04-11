---
name: recall-codex-threads
description: >-
  Search, resolve, and read past Codex conversation threads from local
  ~/.codex/sessions/ and ~/.codex/archived_sessions/ archives via the
  `codex-threads` CLI. Use whenever you want to find an earlier Codex session
  by topic, recover decisions made in prior threads, or mine successful past
  work for reusable patterns. Returns stable JSON with `--json`. Read-only
  over the source archives.
---

# recall-codex-threads

`codex-threads` is a local CLI that indexes `~/.codex/sessions/**/*.jsonl` and
`~/.codex/archived_sessions/**/*.jsonl` into a SQLite + FTS5 derived index and
exposes precise read/search/list commands with a deterministic JSON envelope.
Use it to find prior conversations by topic, to look up exactly what was
decided in a past thread, or to mine old sessions for reusable patterns —
without loading raw transcripts into context.

## When to use

- User says "find that Codex thread where we…", "what did we decide about…",
  "remember the Codex session from last week about…", "show me past work on…",
  "recall the conversation about…"
- Before resuming work on a topic, to recover context from a prior Codex session
- To quote a specific past message by exact id
- To inspect the full event stream of one past thread

## Core conventions

- **Always pass `--json`** unless a human-readable dump is actually wanted.
  Every read/search/list returns a stable envelope:
  `{ schema_version, command, ok, data, meta, error }`.
- **IDs are stable.** `thread_id` is the session UUID parsed from the JSONL
  filename. `message_id` is `{thread_id}:m:{ordinal}`. `event_id` is
  `{thread_id}:e:{ordinal}`. Ordinals are assigned in on-disk order during
  parsing and are stable across syncs of the same underlying file.
- **Exit codes are stable.** `0` ok, `2` usage error, `3` archive not found,
  `4` index missing, `5` not found, `6` ambiguous, `7` sync failed.
- **Default scope excludes subagents.** Search results are always filtered to
  `default_scope = 1`, which hides threads whose `session_meta.source` is a
  subagent spawn (e.g. `subagent:...`) or a non-string object source. Read
  commands (`threads read`, `messages read`, `events read`) work on any
  indexed thread regardless of scope, as long as you already know the id.
- **Lazy auto-sync.** Read commands sync the index automatically when stale.
  Explicit `sync` is only needed before measuring `index stats`.
- **Concurrency-safe reads.** If another `codex-threads` process holds the
  SQLite write lock, read commands fall back to the existing index instead
  of failing.

## Command surface

```text
Query, search, resolve, and read local Codex thread archives with deterministic JSON, predictable
errors, and agent-friendly subcommands.

Usage: codex-threads [OPTIONS] <COMMAND>

Commands:
  sync      Refresh the local derived index from Codex archives
  projects  List indexed projects
  threads   Search, resolve, and read normalized threads
  messages  Search and read normalized messages
  events    Read normalized event streams for a thread
  index     Inspect index statistics
  debug     Show resolved archive and index paths
  help      Print this message or the help of the given subcommand(s)

Options:
      --json
          Emit machine-readable JSON to stdout

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  codex-threads --json sync
  codex-threads --json projects list
  codex-threads --json threads search "build a CLI" --limit 20
  codex-threads --json threads search "refactor index" --project /Users/me/Projects/sweatshop
  codex-threads --json threads resolve "tweet idea"
  codex-threads --json threads read <thread-id>
  codex-threads --json messages search "archive format" --project /Users/me/Projects/sweatshop
  codex-threads --json events read <thread-id> --limit 50
```

Shorthand of the most useful invocations:

```text
codex-threads --json sync [--rebuild]
codex-threads --json projects list [--limit 50]
codex-threads --json threads search <query> [--project <slug-or-cwd>] [--limit 20]
codex-threads --json threads resolve <query>
codex-threads --json threads read <thread-id>
codex-threads --json messages search <query> [--project <slug-or-cwd>] [--limit 20]
codex-threads --json messages read <message-id>
codex-threads --json events read <thread-id> [--limit 50]
codex-threads --json index stats
codex-threads --json debug paths
```

## Examples

Find a past thread by topic:

```bash
codex-threads --json threads search "build a CLI"
```

List recent projects, then narrow a search to one workspace:

```bash
codex-threads --json projects list
codex-threads --json threads search "retry policy" --project /Users/me/Projects/sweatshop
```

Read the full normalized record for one thread:

```bash
codex-threads --json threads read 8069f1c6-8305-4f3d-bc31-6ec5230f8921
```

Resolve a fuzzy reference. On a single hit the thread comes back; on multiple
matches, exit code `6` is returned with a `candidates` array in `error.details`:

```bash
codex-threads --json threads resolve "design doctrine"
```

Find a specific message across all threads, then read it exactly:

```bash
codex-threads --json messages search "retry policy" --limit 5
codex-threads --json messages read 8069f1c6-8305-4f3d-bc31-6ec5230f8921:m:42
```

Walk the raw event stream for one thread (full record payloads, byte-accurate):

```bash
codex-threads --json events read 8069f1c6-8305-4f3d-bc31-6ec5230f8921 --limit 50
```

Inspect index health and source paths:

```bash
codex-threads --json index stats
codex-threads --json debug paths
```

## Composing with jq

Search hits return small records — pipe through `jq` to grab just what you need:

```bash
# Get the top thread id for a query
codex-threads --json threads search "rework plan" --limit 1 | jq -r '.data.items[0].thread_id'

# Get all message ids that match a phrase
codex-threads --json messages search "sandbox policy" --limit 20 | jq -r '.data.items[].message_id'

# Search only within one derived project
codex-threads --json threads search "rework plan" --project /Users/me/Projects/sweatshop | jq '.data.items[] | {thread_id, project_slug}'

# Get the title, source_kind, and cwd for a thread you already know
codex-threads --json threads read <thread-id> | jq '.data.thread | {title, source_kind, cwd}'
```

## Things worth knowing

- **Thread ids come from filenames.** Each session JSONL file is named after
  its session UUID; `thread_id` is the last 36 characters of the filename
  stem. Both live sessions and `archived_sessions/**` are indexed, and the
  `archived` boolean on a thread record tells them apart.
- **Titles come from `session_index.jsonl`.** Codex maintains a top-level
  `~/.codex/session_index.jsonl` with `{id, thread_name, updated_at}` rows.
  The indexer prefers `thread_name` from that file; if a session has no
  matching row, it falls back to a snippet of the first user message, and
  finally to the session UUID itself.
- **Projects are derived from `cwd`, not archive paths.** Codex stores
  sessions under date-based `sessions/` and `archived_sessions/` trees, so
  `codex-threads` derives `project_cwd` from `session_meta.payload.cwd`
  (falling back to the first primary `turn_context.payload.cwd`) and turns it
  into a stable, collision-free `project_slug` using an escaped encoding of
  the normalized `cwd`.
- **Project filters resolve like Claude's.** `--project` accepts an exact
  `project_slug`, an exact `project_cwd`, or a unique substring of either.
  Zero matches return `not_found`; multiple matches return `ambiguous` with
  candidate slugs.
- **Subagent threads are classified, not dropped.** A JSONL file whose
  `session_meta.source` is a subagent object (e.g. `{"subagent": "..."}`)
  gets `default_scope = 0` and `source_kind = "subagent:<kind>"`. Search
  hides these by default; reads still work on them if you know the id.
- **Foreign `session_meta` rows signal nested subagents.** When a JSONL file
  contains `session_meta` records whose `payload.id` does not match the
  file's own thread id, the parser marks `has_subagents = true` and may
  narrow message/event capture to the most recent turn block
  (`turn_context` / `task_started`) instead of the full file.
- **Messages are extracted from two record types.** `event_msg` with payload
  type `user_message` or `agent_message` produces user/assistant messages
  directly. `response_item` records of type `message` with role `assistant`
  also produce assistant messages, with text pulled from `content[].text`
  entries. Other record types are indexed as events but not as messages.
- **Events store byte offsets, not payloads.** The `events` table records
  `file_path`, `byte_start`, and `byte_len` for each line. `events read`
  rehydrates payloads by seeking into the source JSONL file — that's why
  source archives must remain on disk and unmodified.
- **Source archives are read-only.** The derived index lives at
  `$CODEX_HOME/codex-threads/index.sqlite` (default
  `~/.codex/codex-threads/index.sqlite`). `sync` only writes to the derived
  index, never to `sessions/` or `archived_sessions/`.
- **`sync --rebuild`** drops and recreates the derived index from scratch.
  Use it if you suspect index corruption or want to pick up a schema change.
