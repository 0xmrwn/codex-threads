# codex-threads

`codex-threads` is a local CLI for querying Codex session archives with stable JSON, predictable errors, and minimal noise.

It is designed for repeated agent use against local `~/.codex` data, not as a hosted service or GUI.

> **Scope.** This is a personal tool published for convenience. No support is
> guaranteed, external contributions aren't accepted, and behavior may change
> between releases.

## What It Does

- Indexes local Codex archives from `~/.codex/sessions/**` and `~/.codex/archived_sessions/**`
- Exposes normalized `threads`, `messages`, and `events` reads
- Supports exact reads by stable ids after discovery/search
- Keeps machine-readable output stable with `--json`

## Install

### 1. Install the CLI

Pinned to a specific version (reproducible across machines):

```bash
cargo install --git https://github.com/0xmrwn/codex-threads --tag v0.1.0 --locked
```

Always-latest (re-run with `--force` to upgrade in place):

```bash
cargo install --git https://github.com/0xmrwn/codex-threads --tag latest --locked --force
```

The `latest` tag is automatically moved to the newest released commit by
[`.github/workflows/move-latest.yml`](.github/workflows/move-latest.yml)
whenever a release is published or edited.

Both install commands build from source — one-time ~20s compile per machine,
no precompiled binaries. After install, run the CLI normally:

```bash
codex-threads --help
```

### 2. Optional: install the skill so agents discover it

`codex-threads` ships a `recall-codex-threads` skill under
[`skills/recall-codex-threads/`](skills/recall-codex-threads/SKILL.md). Install
it globally into any supported agent with [`vercel-labs/skills`](https://github.com/vercel-labs/skills):

```bash
npx skills add 0xmrwn/codex-threads -g -a claude-code -a codex -y
```

The same SKILL.md works for Claude Code, Codex, and the other 40+ agents
`skills` supports — the package symlinks a single canonical copy into each
agent's skills dir, so one `npx skills update` later propagates to all of
them at once. Skill install and CLI install are independent; you can have
either without the other.

## Common Commands

```bash
codex-threads --json sync
codex-threads --json threads search "build a CLI" --limit 20
codex-threads --json threads resolve "tweet idea"
codex-threads --json threads read <thread-id>
codex-threads --json messages search "archive format" --limit 20
codex-threads --json messages read <message-id>
codex-threads --json events read <thread-id> --limit 50
codex-threads --json index stats
codex-threads --json debug paths
```

## Behavior

- Source archives are read-only
- The derived index lives under `$CODEX_HOME/codex-threads/index.sqlite` or `~/.codex/codex-threads/index.sqlite`
- `sync` is explicit, but read/search commands also auto-sync when the index is missing or stale
- If another `codex-threads` process is already syncing, read commands fall back to the current index instead of failing on a write lock

## Output Contract

Every `--json` command writes a single JSON envelope to stdout with:

- `schema_version`
- `command`
- `ok`
- `data`
- `meta`
- `error`

Diagnostics and warnings go to stderr.

## Development

```bash
cargo test
cargo fmt
```
