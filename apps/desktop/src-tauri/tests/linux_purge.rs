//! Behavioral and lifecycle tests for Linux safe purge and state preservation.

#![cfg(unix)]

mod support;

use std::fs;
use std::path::PathBuf;

use shaprint_desktop::adapters::daemon::{parse_cli_args, CliCommand};
use shaprint_desktop::adapters::printers::cups::CupsDestinationRecord;
use shaprint_desktop::adapters::purge::{
    execute_purge, format_purge_report, PurgeExecutionOptions,
};
use shaprint_desktop::domain::ErrorCode;

fn temporary_directory(name: &str) -> PathBuf {
    static TOOLS: std::sync::Once = std::sync::Once::new();
    TOOLS.call_once(|| {
        use std::os::unix::fs::PermissionsExt;

        // These filesystem fixtures must not stop the developer's daemon or request
        // authorization from the host's systemd/firewall services.
        let tools =
            std::env::temp_dir().join(format!("shaprint-purge-tools-{}", std::process::id()));
        fs::create_dir_all(&tools).unwrap();
        for name in ["systemctl", "ufw"] {
            let tool = tools.join(name);
            fs::write(&tool, "#!/bin/sh\nexit 1\n").unwrap();
            fs::set_permissions(tool, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut paths = vec![tools];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    });
    support::temporary_directory(name)
}

#[test]
fn cli_parses_purge_with_and_without_confirm_and_yes_flags() {
    let no_flag = parse_cli_args(&["shaprintctl", "purge"]).expect("parses purge without flags");
    assert_eq!(no_flag.command, CliCommand::Purge { confirm: false });

    let with_confirm =
        parse_cli_args(&["shaprintctl", "purge", "--confirm"]).expect("parses purge --confirm");
    assert_eq!(with_confirm.command, CliCommand::Purge { confirm: true });

    let with_yes = parse_cli_args(&["shaprintctl", "purge", "--yes"]).expect("parses purge --yes");
    assert_eq!(with_yes.command, CliCommand::Purge { confirm: true });

    let with_socket = parse_cli_args(&[
        "shaprintctl",
        "--socket",
        "/tmp/custom.sock",
        "purge",
        "--confirm",
    ])
    .expect("parses purge with socket");
    assert_eq!(with_socket.socket_path, PathBuf::from("/tmp/custom.sock"));
    assert_eq!(with_socket.command, CliCommand::Purge { confirm: true });
}

#[tokio::test]
async fn purge_refuses_without_confirm() {
    let dir = temporary_directory("purge-no-confirm");
    let options = PurgeExecutionOptions {
        confirm: false,
        config_dir: dir.join("etc/shaprint"),
        credential_dir: dir.join("etc/shaprint-credentials"),
        state_dir: dir.join("var/lib/shaprint"),
        socket_path: dir.join("run/shaprint/shaprintd.sock"),
        cups_endpoint: None,
        proxy_port: 48632,
        firewall_program: Some(dir.join("firewall-not-installed")),
        require_root: false,
    };

    let err = execute_purge(&options)
        .await
        .expect_err("purge without confirm must fail");
    assert_eq!(err.code(), ErrorCode::InvalidInput);
    assert!(err.message().contains("--confirm"));
}

#[tokio::test]
async fn purge_refuses_unauthorized_non_root_when_enforced() {
    let dir = temporary_directory("purge-non-root");
    let options = PurgeExecutionOptions {
        confirm: true,
        config_dir: dir.join("etc/shaprint"),
        credential_dir: dir.join("etc/shaprint-credentials"),
        state_dir: dir.join("var/lib/shaprint"),
        socket_path: dir.join("run/shaprint/shaprintd.sock"),
        cups_endpoint: None,
        proxy_port: 48632,
        firewall_program: Some(dir.join("firewall-not-installed")),
        require_root: true, // will check current effective uid
    };

    // If running as non-root (e.g. normal user in cargo test), must fail with NotAuthorized
    if std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        != Some(0)
    {
        let err = execute_purge(&options)
            .await
            .expect_err("purge as non-root must fail");
        assert_eq!(err.code(), ErrorCode::NotAuthorized);
    }
}

#[tokio::test]
async fn purge_removes_state_credentials_config_and_socket_idempotently() {
    let root = temporary_directory("purge-state-removal");
    let config_dir = root.join("etc/shaprint");
    let credential_dir = root.join("etc/shaprint-credentials");
    let state_dir = root.join("var/lib/shaprint");
    let run_dir = root.join("run/shaprint");
    let socket_path = run_dir.join("shaprintd.sock");

    fs::create_dir_all(&config_dir).expect("creates config dir");
    fs::create_dir_all(&credential_dir).expect("creates credential dir");
    fs::create_dir_all(&state_dir).expect("creates state dir");
    fs::create_dir_all(&run_dir).expect("creates run dir");

    fs::write(config_dir.join("config.json"), "{}").expect("writes config");
    fs::write(
        credential_dir.join("network-channel"),
        "mock-client-credentials",
    )
    .expect("writes mock credentials");
    fs::write(state_dir.join("server-identity.key.der"), b"key").expect("writes key");
    fs::write(state_dir.join("server-identity.cert.der"), b"cert").expect("writes cert");
    fs::write(state_dir.join("sharing_state.json"), "[]").expect("writes sharing");
    fs::write(state_dir.join("client-server-trust.json"), "[]").expect("writes trust");
    fs::write(&socket_path, "").expect("writes socket stub");

    let options = PurgeExecutionOptions {
        confirm: true,
        config_dir: config_dir.clone(),
        credential_dir: credential_dir.clone(),
        state_dir: state_dir.clone(),
        socket_path: socket_path.clone(),
        cups_endpoint: None,
        proxy_port: 48632,
        firewall_program: Some(root.join("firewall-not-installed")),
        require_root: false,
    };

    let report = shaprint_desktop::adapters::purge::execute_purge_with_destinations_and_lpadmin(
        &options,
        Some(vec![]),
        None,
    )
    .await
    .expect("purge succeeds");
    assert!(report.purged);
    assert!(report.removed_credentials);
    assert!(report.removed_identity_and_state);
    assert!(report.removed_config);

    // Verify filesystem state
    assert!(!credential_dir.join("network-channel").exists());
    assert!(!state_dir.join("server-identity.key.der").exists());
    assert!(!state_dir.join("server-identity.cert.der").exists());
    assert!(!state_dir.join("sharing_state.json").exists());
    assert!(!state_dir.join("client-server-trust.json").exists());
    assert!(!config_dir.join("config.json").exists());
    assert!(!socket_path.exists());

    // Idempotent second purge
    let second_report =
        shaprint_desktop::adapters::purge::execute_purge_with_destinations_and_lpadmin(
            &options,
            Some(vec![]),
            None,
        )
        .await
        .expect("second purge is idempotent");
    assert!(second_report.purged);
    assert!(second_report.warnings.is_empty());
    assert!(second_report.recovery_actions.is_empty());
}

#[tokio::test]
async fn purge_removes_only_shaprint_client_queues_and_preserves_local_and_foreign_queues() {
    let root = temporary_directory("purge-cups-queues");
    fs::create_dir_all(&root).expect("creates root dir");
    let fake_lpadmin = root.join("fake-lpadmin.sh");
    let deleted_log = root.join("deleted_queues.log");

    fs::write(
        &fake_lpadmin,
        format!(
            r#"#!/bin/sh
if [ "$1" = "-x" ]; then
    echo "$2" >> "{}"
    exit 0
fi
exit 0
"#,
            deleted_log.display()
        ),
    )
    .expect("writes fake lpadmin");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_lpadmin, fs::Permissions::from_mode(0o755)).unwrap();
    }

    use shaprint_desktop::domain::{ClientQueueRequest, PrinterName};

    let req = ClientQueueRequest::new("192.168.1.10", PrinterName::parse("Office_HP").unwrap())
        .expect("valid client queue request");
    let shaprint_cups_name = req.cups_destination_name();

    // Set up mock destinations
    let mock_destinations = vec![
        CupsDestinationRecord {
            name: shaprint_cups_name.clone(),
            device_uri: "ipp://127.0.0.1:48632/ipp/print/192.168.1.10/Office_HP".to_string(),
            printer_info: "Office_HP (192.168.1.10)".to_string(),
            state: Some(3),
            accepting_jobs: true,
            make_and_model: "Generic IPP Everywhere Printer".to_string(),
            has_usable_conversion: true,
            options: Vec::new(),
        },
        CupsDestinationRecord {
            name: "local_laser_printer".to_string(),
            device_uri: "usb://HP/LaserJet%201020".to_string(),
            printer_info: "Physical Local Office Printer".to_string(),
            state: Some(3),
            accepting_jobs: true,
            make_and_model: "HP LaserJet 1020".to_string(),
            has_usable_conversion: true,
            options: Vec::new(),
        },
        CupsDestinationRecord {
            name: "foreign_ipp_queue".to_string(),
            device_uri: "ipp://external.cups.server:631/printers/accounting".to_string(),
            printer_info: "Corporate CUPS Queue".to_string(),
            state: Some(3),
            accepting_jobs: true,
            make_and_model: "Generic PostScript Printer".to_string(),
            has_usable_conversion: true,
            options: Vec::new(),
        },
    ];

    let options = PurgeExecutionOptions {
        confirm: true,
        config_dir: root.join("etc/shaprint"),
        credential_dir: root.join("etc/shaprint-credentials"),
        state_dir: root.join("var/lib/shaprint"),
        socket_path: root.join("run/shaprint/shaprintd.sock"),
        cups_endpoint: None,
        proxy_port: 48632,
        firewall_program: Some(root.join("firewall-not-installed")),
        require_root: false,
    };

    let report = shaprint_desktop::adapters::purge::execute_purge_with_destinations_and_lpadmin(
        &options,
        Some(mock_destinations),
        Some(&fake_lpadmin),
    )
    .await
    .expect("purge succeeds");

    assert_eq!(
        report.removed_client_queues,
        vec![shaprint_cups_name.clone()]
    );

    let deleted = fs::read_to_string(&deleted_log).unwrap_or_default();
    assert!(deleted.contains(&shaprint_cups_name));
    assert!(!deleted.contains("local_laser_printer"));
    assert!(!deleted.contains("foreign_ipp_queue"));
}

#[tokio::test]
async fn purge_handles_partial_external_failures_and_reports_recovery_actions() {
    let root = temporary_directory("purge-partial-failure");
    fs::create_dir_all(&root).expect("creates root dir");
    let failing_lpadmin = root.join("failing-lpadmin.sh");

    fs::write(
        &failing_lpadmin,
        r#"#!/bin/sh
echo "lpadmin: client-error-not-authorized" >&2
exit 1
"#,
    )
    .expect("writes failing lpadmin");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&failing_lpadmin, fs::Permissions::from_mode(0o755)).unwrap();
    }

    use shaprint_desktop::domain::{ClientQueueRequest, PrinterName};

    let req = ClientQueueRequest::new("192.168.1.10", PrinterName::parse("Fail_Printer").unwrap())
        .expect("valid client queue request");
    let shaprint_cups_name = req.cups_destination_name();

    let mock_destinations = vec![CupsDestinationRecord {
        name: shaprint_cups_name,
        device_uri: "ipp://127.0.0.1:48632/ipp/print/192.168.1.10/Fail_Printer".to_string(),
        printer_info: "Fail_Printer (192.168.1.10)".to_string(),
        state: Some(3),
        accepting_jobs: true,
        make_and_model: "Generic IPP Everywhere Printer".to_string(),
        has_usable_conversion: true,
        options: Vec::new(),
    }];

    let options = PurgeExecutionOptions {
        confirm: true,
        config_dir: root.join("etc/shaprint"),
        credential_dir: root.join("etc/shaprint-credentials"),
        state_dir: root.join("var/lib/shaprint"),
        socket_path: root.join("run/shaprint/shaprintd.sock"),
        cups_endpoint: None,
        proxy_port: 48632,
        firewall_program: Some(root.join("firewall-not-installed")),
        require_root: false,
    };

    let report = shaprint_desktop::adapters::purge::execute_purge_with_destinations_and_lpadmin(
        &options,
        Some(mock_destinations),
        Some(&failing_lpadmin),
    )
    .await
    .expect("purge completes with partial warnings");

    assert!(!report.warnings.is_empty());
    assert!(!report.recovery_actions.is_empty());
    assert!(report
        .recovery_actions
        .iter()
        .any(|a| a.contains("lpadmin -x")));

    // Format check
    let human = format_purge_report(&report, false).expect("formats human");
    assert!(human.contains("warnings:"));
    assert!(human.contains("recovery:"));

    let json = format_purge_report(&report, true).expect("formats json");
    assert!(json.contains("\"warnings\":"));
}
