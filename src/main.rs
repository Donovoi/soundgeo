#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod cli;
mod ui;

fn main() -> std::process::ExitCode {
    // Keep double-click launches console-free, while preserving legacy CLI access.
    #[cfg(windows)]
    unsafe {
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        if std::env::args_os().len() > 1 {
            AttachConsole(u32::MAX);
        }
    }
    cli::entry(true)
}
