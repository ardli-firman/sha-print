// Keep the console window hidden on Windows release builds; debug builds keep stdout for logs.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use shaprint_desktop::adapters::elevation::{parse_command_line, run_elevated, CommandLine};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    match parse_command_line(&arguments) {
        // The elevated setup helper: one configuration action, then exit. The exit code is what the
        // app reads to report whether the user granted permission.
        CommandLine::Setup(action) => {
            if let Err(error) = run_elevated(action) {
                eprintln!("setup failed code={} message={error}", error.code_str());
                std::process::exit(1);
            }
        }
        CommandLine::Invalid(value) => {
            eprintln!("unknown setup action '{value}'");
            std::process::exit(2);
        }
        CommandLine::Run => {
            if let Err(error) = shaprint_desktop::run() {
                log::error!(
                    "desktop shell exited code={} message={}",
                    error.code_str(),
                    error
                );
                std::process::exit(1);
            }
        }
    }
}
