// Keep the console window hidden on Windows release builds; debug builds keep stdout for logs.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    if let Err(error) = shaprint_desktop::run() {
        log::error!(
            "desktop shell exited code={} message={}",
            error.code_str(),
            error
        );
        std::process::exit(1);
    }
}
