//! `shaprintctl`: local CLI for observing and controlling `shaprintd` over its Unix socket.

#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    use shaprint_desktop::adapters::daemon::{
        format_runtime_status, parse_cli_args, CliCommand, DaemonClient,
    };

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

    let client = DaemonClient::new(&options.socket_path);
    let outcome = match &options.command {
        CliCommand::Status => client.status().await,
        CliCommand::StartService(id) => client.start_service(id.as_str()).await,
        CliCommand::StopService(id) => client.stop_service(id.as_str()).await,
        CliCommand::Shutdown => client.shutdown().await,
    };

    match outcome {
        Ok(status) => match format_runtime_status(&status, options.json) {
            Ok(rendered) => print!("{rendered}"),
            Err(error) => {
                eprintln!(
                    "shaprintctl error code={} message={}",
                    error.code_str(),
                    error
                );
                std::process::exit(1);
            }
        },
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
