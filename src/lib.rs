mod archive;
mod cli;
mod envelope;
mod error;
mod index;
mod paths;

pub fn run() -> i32 {
    cli::run()
}
