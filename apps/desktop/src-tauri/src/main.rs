// Keep the console window hidden on Windows release builds; debug builds keep stdout for logs.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use shaprint_desktop::adapters::elevation::{parse_command_line, run_elevated, CommandLine};
use shaprint_desktop::adapters::startup::launches_in_background;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    match parse_command_line(&arguments) {
        // The elevated setup helper: one setup request, then exit. The exit code carries the
        // classified failure back to the app, which turns it into the advice the user sees; the
        // detail stays here on standard error (ADR 0005).
        CommandLine::Setup(request) => {
            if let Err(failure) = run_elevated(request) {
                eprintln!(
                    "setup failed kind={} detail={}",
                    failure.kind().id(),
                    failure.detail()
                );
                std::process::exit(failure.kind().exit_code());
            }
        }
        CommandLine::Invalid(value) => {
            // A malformed helper request is a bug in the app rather than a classified setup
            // failure, so it reports a code the app treats as unexpected.
            eprintln!("invalid setup request: {value}");
            std::process::exit(2);
        }
        CommandLine::Run => {
            // A login launch starts the background services and waits in the tray; an interactive
            // launch opens the window as usual (#40).
            let background = launches_in_background(&arguments);
            if let Err(error) = shaprint_desktop::run(background) {
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
