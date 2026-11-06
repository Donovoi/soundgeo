// A console-subsystem entry point, sharing every operation with the desktop app.
#[path = "../cli.rs"]
mod cli;
#[path = "../ui.rs"]
mod ui;

fn main() -> std::process::ExitCode {
    cli::entry(false)
}
