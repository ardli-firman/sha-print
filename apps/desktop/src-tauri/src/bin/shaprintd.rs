//! `shaprintd`: the Linux machine daemon supervising the Client proxy and Server runtime.

#[cfg(unix)]
struct JournalStderrLogger;

#[cfg(unix)]
impl log::Log for JournalStderrLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "shaprintd level={} target={} {}",
                record.level().as_str().to_ascii_lowercase(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

#[cfg(unix)]
static LOGGER: JournalStderrLogger = JournalStderrLogger;

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use shaprint_desktop::adapters::daemon::{parse_daemon_args, DaemonServer};

    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }

    let arguments: Vec<String> = std::env::args().collect();
    let config = match parse_daemon_args(&arguments) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "shaprintd error code={} message={}",
                error.code_str(),
                error
            );
            std::process::exit(2);
        }
    };

    let server = match DaemonServer::launch(config).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!(
                "shaprintd failed to start code={} message={}",
                error.code_str(),
                error
            );
            std::process::exit(1);
        }
    };

    let shutdown_handle = server.shutdown_handle();
    drop(tokio::spawn(async move {
        wait_for_termination_signal().await;
        shutdown_handle.request_shutdown();
    }));

    if let Err(error) = server.wait().await {
        eprintln!(
            "shaprintd shutdown failed code={} message={}",
            error.code_str(),
            error
        );
        std::process::exit(1);
    }
}

#[cfg(unix)]
async fn wait_for_termination_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    match signal(SignalKind::terminate()) {
        Ok(mut sigterm) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("shaprintd is only supported on Linux");
    std::process::exit(1);
}
