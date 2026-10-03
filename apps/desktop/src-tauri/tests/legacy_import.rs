//! Moving settings from the previous .NET application, through the real file reader and the real
//! Network Channel storage (#41).
//!
//! The observable promises: the previous app's files are only read, printer queues are never
//! activated, no server certificate is trusted, and repeating the import changes nothing.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use shaprint_desktop::adapters::ipps::NetworkChannel;
use shaprint_desktop::adapters::legacy::FileLegacySettings;
use shaprint_desktop::application::{ChannelOutcome, ChannelStore, LegacyImport};
use shaprint_desktop::Shell;

/// The previous application's folder, which has to exist before fixtures are written into it.
fn legacy_directory(name: &str) -> PathBuf {
    let directory = support::temporary_directory(name);
    std::fs::create_dir_all(&directory).expect("creates the previous app's folder");
    directory
}

/// This app's own data folder.
fn data_directory(name: &str) -> PathBuf {
    support::temporary_directory(name)
}

fn write(directory: &Path, name: &str, contents: &str) {
    std::fs::write(directory.join(name), contents).expect("writes the fixture");
}

/// The previous application as a user who installed client queues and shared server queues.
fn previous_install(directory: &Path) {
    write(
        directory,
        "AppSettings.json",
        r#"{"AutoUpdateEnabled":true,"AutoPurgeEnabled":true,"Channel":1,"AutoSaveScans":false,"EncryptedNetworkChannel":""}"#,
    );
    write(
        directory,
        "ClientConfig.json",
        r#"[{"VirtualPrinterName":"ShaPrint Office Printer","PipeName":"shaprint-1","ServerIp":"192.0.2.10","TargetPrinterName":"Office Printer","DriverName":"MS Publisher Imagesetter"}]"#,
    );
    write(
        directory,
        "ServerConfig.json",
        r#"{"Printers":["HP LaserJet"]}"#,
    );
}

#[tokio::test]
async fn a_previous_install_is_reported_without_activating_its_queues() {
    let legacy = legacy_directory("legacy");
    let data = data_directory("data");
    previous_install(&legacy);
    let settings_before = std::fs::read_to_string(legacy.join("AppSettings.json")).expect("reads");

    let channel = Arc::new(NetworkChannel::open(&data).expect("opens this app's storage"));
    let import = LegacyImport::new(
        Arc::new(FileLegacySettings::in_directory(&legacy)),
        Arc::clone(&channel) as Arc<dyn ChannelStore>,
        Some(&data),
    );

    let report = import.apply().await.expect("imports");

    assert!(report.found);
    // The previous app never set a Network Channel of its own, so nothing is carried over.
    assert_eq!(report.channel, ChannelOutcome::Absent);
    assert!(!channel.is_configured());
    assert!(report.queues_need_reselection);
    for key in ["client-queues", "shared-queues"] {
        let setting = report
            .settings
            .iter()
            .find(|setting| setting.key == key)
            .unwrap_or_else(|| panic!("the report never mentions {key}"));
        // Every setting the report names is one this app left behind, with the reason why.
        assert!(
            setting.reason.contains("again"),
            "{key}: {}",
            setting.reason
        );
    }

    // The previous app keeps working: its files are unchanged and nothing was added to them.
    assert_eq!(
        std::fs::read_to_string(legacy.join("AppSettings.json")).expect("reads"),
        settings_before
    );
    assert!(!legacy.join("legacy-import.json").exists());
    // No queue was installed and no server was trusted by importing.
    assert!(!data.join("network-channel-verifier.json").exists());
    assert!(!data.join("client-server-trust.json").exists());

    let _ = std::fs::remove_dir_all(&legacy);
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn repeating_the_import_changes_nothing() {
    let legacy = legacy_directory("repeat-legacy");
    let data = data_directory("repeat-data");
    previous_install(&legacy);

    let channel = Arc::new(NetworkChannel::open(&data).expect("opens this app's storage"));
    let import = LegacyImport::new(
        Arc::new(FileLegacySettings::in_directory(&legacy)),
        Arc::clone(&channel) as Arc<dyn ChannelStore>,
        Some(&data),
    );

    let first = import.apply().await.expect("imports");
    let second = import.apply().await.expect("imports again");

    assert_eq!(first, second);
    assert!(!channel.is_configured());
    assert!(!data.join("client-server-trust.json").exists());

    let _ = std::fs::remove_dir_all(&legacy);
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn a_computer_without_the_previous_app_imports_nothing() {
    let legacy = legacy_directory("no-legacy");
    let data = data_directory("no-legacy-data");

    let channel = Arc::new(NetworkChannel::open(&data).expect("opens this app's storage"));
    let import = LegacyImport::new(
        Arc::new(FileLegacySettings::in_directory(&legacy)),
        Arc::clone(&channel) as Arc<dyn ChannelStore>,
        Some(&data),
    );

    let report = import.apply().await.expect("imports");

    assert!(!report.found);
    assert!(report.settings.is_empty());
    assert!(!report.queues_need_reselection);

    let _ = std::fs::remove_dir_all(&legacy);
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn starting_the_shell_neither_invents_a_channel_nor_trusts_a_server() {
    let data = data_directory("shell-start");

    let shell = Shell::new(&data).expect("builds the shell");
    // What a launch does before the background services start.
    let report = shell.legacy_import().apply().await.expect("imports");

    // This app only ends up with a channel when the import says it took one. (On a Windows machine
    // that really has the previous app installed, this is the import doing its job.)
    assert_eq!(
        shell.network_channel().is_configured(),
        report.channel == ChannelOutcome::Imported
    );
    // Importing never approves a server: that stays an explicit user decision.
    assert!(!data.join("client-server-trust.json").exists());
    shell.runtime().shutdown().await.expect("shuts down");

    let _ = std::fs::remove_dir_all(&data);
}
