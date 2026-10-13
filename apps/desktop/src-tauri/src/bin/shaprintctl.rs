//! `shaprintctl`: local CLI for observing and controlling `shaprintd` over its Unix socket.

#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    use shaprint_desktop::adapters::daemon::{execute_cli_command, parse_cli_args};

    let arguments: Vec<String> = std::env::args().collect();
    let options = match parse_cli_args(&arguments) {
        Ok(options) => options,
        Err(error) => {
            eprintln!(
                "shaprintctl error code={} message={}",
                error.code_str(),
                error
            );
            std::process::exit(2);
        }
    };

    match execute_cli_command(&options).await {
        Ok(rendered) => print!("{rendered}"),
        Err(error) => {
            eprintln!(
                "shaprintctl failed code={} message={}",
                error.code_str(),
                error
            );
            std::process::exit(1);
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("shaprintctl is only supported on Linux");
    std::process::exit(1);
}
