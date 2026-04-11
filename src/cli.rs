use crate::envelope::Envelope;
use crate::error::{AppError, ErrorCode};
use crate::index;
use crate::paths::ResolvedPaths;
use clap::{ArgAction, Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    name = "codex-threads",
    version,
    about = "Query local Codex conversation archives with stable JSON.",
    long_about = "Query, search, resolve, and read local Codex thread archives with deterministic JSON, predictable errors, and agent-friendly subcommands.",
    after_help = "Examples:\n  codex-threads --json sync\n  codex-threads --json threads search \"build a CLI\" --limit 20\n  codex-threads --json threads resolve \"tweet idea\"\n  codex-threads --json threads read <thread-id>\n  codex-threads --json events read <thread-id> --limit 50"
)]
pub struct Cli {
    #[arg(long, global = true, action = ArgAction::SetTrue, help = "Emit machine-readable JSON to stdout")]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Refresh the local derived index from Codex archives")]
    Sync(SyncArgs),
    #[command(subcommand)]
    #[command(about = "Search, resolve, and read normalized threads")]
    Threads(ThreadCommand),
    #[command(subcommand)]
    #[command(about = "Search and read normalized messages")]
    Messages(MessageCommand),
    #[command(subcommand)]
    #[command(about = "Read normalized event streams for a thread")]
    Events(EventCommand),
    #[command(subcommand)]
    #[command(about = "Inspect index statistics")]
    Index(IndexCommand),
    #[command(subcommand)]
    #[command(about = "Show resolved archive and index paths")]
    Debug(DebugCommand),
}

#[derive(Debug, Args)]
struct SyncArgs {
    #[arg(long, action = ArgAction::SetTrue, help = "Rebuild the derived index from scratch")]
    rebuild: bool,
}

#[derive(Debug, Subcommand)]
enum ThreadCommand {
    #[command(about = "Search normalized top-level threads")]
    Search(SearchArgs),
    #[command(about = "Resolve a fuzzy thread reference to one exact thread id")]
    Resolve(ResolveArgs),
    #[command(about = "Read one exact thread by stable thread id")]
    Read(ReadThreadArgs),
}

#[derive(Debug, Subcommand)]
enum MessageCommand {
    #[command(about = "Search normalized top-level messages")]
    Search(SearchArgs),
    #[command(about = "Read one exact message by stable message id")]
    Read(ReadMessageArgs),
}

#[derive(Debug, Subcommand)]
enum EventCommand {
    #[command(about = "Read the event stream for one exact thread id")]
    Read(ReadEventsArgs),
}

#[derive(Debug, Subcommand)]
enum IndexCommand {
    #[command(about = "Show index counts, source roots, and last sync time")]
    Stats,
}

#[derive(Debug, Subcommand)]
enum DebugCommand {
    #[command(about = "Show resolved archive discovery and index paths")]
    Paths,
}

#[derive(Debug, Args)]
struct SearchArgs {
    #[arg(help = "Search query")]
    query: String,
    #[arg(
        long,
        default_value_t = 20,
        help = "Maximum number of results to return"
    )]
    limit: usize,
}

#[derive(Debug, Args)]
struct ResolveArgs {
    #[arg(help = "Thread id, exact title, or fuzzy thread reference")]
    query: String,
}

#[derive(Debug, Args)]
struct ReadThreadArgs {
    #[arg(help = "Stable thread id")]
    thread_id: String,
}

#[derive(Debug, Args)]
struct ReadMessageArgs {
    #[arg(help = "Stable message id")]
    message_id: String,
}

#[derive(Debug, Args)]
struct ReadEventsArgs {
    #[arg(help = "Stable thread id")]
    thread_id: String,
    #[arg(
        long,
        default_value_t = 50,
        help = "Maximum number of events to return"
    )]
    limit: usize,
}

pub fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return error.exit_code();
        }
    };
    let json_mode = cli.json;

    let paths = match ResolvedPaths::discover() {
        Ok(paths) => paths,
        Err(error) => return emit_error("init", &error, json_mode, None),
    };

    match dispatch(cli, &paths) {
        Ok(code) => code,
        Err((command, error, auto_sync)) => emit_error(&command, &error, json_mode, auto_sync),
    }
}

fn dispatch(cli: Cli, paths: &ResolvedPaths) -> Result<i32, (String, AppError, Option<bool>)> {
    match cli.command {
        Command::Sync(args) => {
            let command = "sync";
            match index::sync(paths, args.rebuild) {
                Ok(summary) => {
                    emit_success(command, cli.json, summary, None);
                    Ok(0)
                }
                Err(error) => Err((command.to_string(), error, None)),
            }
        }
        Command::Threads(command) => match command {
            ThreadCommand::Search(args) => {
                let command = "threads search";
                match index::search_threads(paths, &args.query, args.limit) {
                    Ok((items, auto_sync)) => {
                        emit_success(
                            command,
                            cli.json,
                            json!({ "items": items, "limit": args.limit }),
                            Some(auto_sync),
                        );
                        Ok(0)
                    }
                    Err(error) => Err((command.to_string(), error, None)),
                }
            }
            ThreadCommand::Resolve(args) => {
                let command = "threads resolve";
                match index::resolve_thread(paths, &args.query) {
                    Ok((thread, auto_sync)) => {
                        emit_success(
                            command,
                            cli.json,
                            json!({ "thread": thread }),
                            Some(auto_sync),
                        );
                        Ok(0)
                    }
                    Err(error) => Err((command.to_string(), error, None)),
                }
            }
            ThreadCommand::Read(args) => {
                let command = "threads read";
                match index::read_thread(paths, &args.thread_id) {
                    Ok((thread, auto_sync)) => {
                        emit_success(
                            command,
                            cli.json,
                            json!({ "thread": thread }),
                            Some(auto_sync),
                        );
                        Ok(0)
                    }
                    Err(error) => Err((command.to_string(), error, None)),
                }
            }
        },
        Command::Messages(command) => match command {
            MessageCommand::Search(args) => {
                let command = "messages search";
                match index::search_messages(paths, &args.query, args.limit) {
                    Ok((items, auto_sync)) => {
                        emit_success(
                            command,
                            cli.json,
                            json!({ "items": items, "limit": args.limit }),
                            Some(auto_sync),
                        );
                        Ok(0)
                    }
                    Err(error) => Err((command.to_string(), error, None)),
                }
            }
            MessageCommand::Read(args) => {
                let command = "messages read";
                match index::read_message(paths, &args.message_id) {
                    Ok((message, auto_sync)) => {
                        emit_success(
                            command,
                            cli.json,
                            json!({ "message": message }),
                            Some(auto_sync),
                        );
                        Ok(0)
                    }
                    Err(error) => Err((command.to_string(), error, None)),
                }
            }
        },
        Command::Events(command) => match command {
            EventCommand::Read(args) => {
                let command = "events read";
                match index::read_events(paths, &args.thread_id, args.limit) {
                    Ok((events, auto_sync)) => {
                        emit_success(
                            command,
                            cli.json,
                            json!({ "items": events, "limit": args.limit }),
                            Some(auto_sync),
                        );
                        Ok(0)
                    }
                    Err(error) => Err((command.to_string(), error, None)),
                }
            }
        },
        Command::Index(command) => match command {
            IndexCommand::Stats => {
                let command = "index stats";
                match index::stats(paths) {
                    Ok((stats, auto_sync)) => {
                        emit_success(command, cli.json, stats, Some(auto_sync));
                        Ok(0)
                    }
                    Err(error) => Err((command.to_string(), error, None)),
                }
            }
        },
        Command::Debug(command) => match command {
            DebugCommand::Paths => {
                let command = "debug paths";
                let payload = json!({
                    "codex_home": paths.codex_home,
                    "sessions_root": paths.sessions_root,
                    "archived_root": paths.archived_root,
                    "session_index_path": paths.session_index_path,
                    "index_dir": paths.index_dir,
                    "index_path": paths.index_path,
                    "sessions_root_exists": paths.sessions_root.exists(),
                    "archived_root_exists": paths.archived_root.exists(),
                    "session_index_exists": paths.session_index_path.exists(),
                    "index_exists": paths.index_path.exists(),
                });
                emit_success(command, cli.json, payload, None);
                Ok(0)
            }
        },
    }
}

fn emit_success<T: Serialize>(command: &str, json_mode: bool, data: T, auto_sync: Option<bool>) {
    if json_mode {
        let envelope = Envelope::success(command, data, auto_sync);
        println!(
            "{}",
            serde_json::to_string_pretty(&envelope).expect("envelope serialization must succeed")
        );
    } else {
        emit_text_success(command, data);
    }
}

fn emit_text_success<T: Serialize>(command: &str, data: T) {
    match command {
        "sync" | "index stats" | "debug paths" | "threads read" | "messages read" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&data).expect("text serialization must succeed")
            );
        }
        "threads search" | "messages search" | "events read" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&data).expect("text serialization must succeed")
            );
        }
        "threads resolve" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&data).expect("text serialization must succeed")
            );
        }
        _ => println!(
            "{}",
            serde_json::to_string_pretty(&data).expect("text serialization must succeed")
        ),
    }
}

fn emit_error(command: &str, error: &AppError, json_mode: bool, auto_sync: Option<bool>) -> i32 {
    if json_mode {
        let envelope = Envelope::failure(command, error, auto_sync);
        println!(
            "{}",
            serde_json::to_string_pretty(&envelope)
                .expect("error envelope serialization must succeed")
        );
    } else {
        eprintln!("{error}");
        if error.code().exit_code() == ErrorCode::Ambiguous.exit_code() {
            if let Some(details) = error.body().details {
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&details)
                        .expect("error detail serialization must succeed")
                );
            }
        }
    }
    error.exit_code()
}
