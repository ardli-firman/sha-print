//! Configuration actions a server user can ask the shell for, and which of them need
//! administrator rights.
//!
//! ADR 0001: setup may request administrator permission once, while routine operation runs as the
//! logged-in user. Keeping the classification in the domain — instead of at each call site — is
//! what makes "elevation only where it is required" checkable.

/// A configuration action the shell can perform for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SetupAction {
    /// Let clients reach the sharing endpoint through the Windows firewall. Machine-wide state, so
    /// Windows asks for administrator permission.
    AllowInboundSharing,
    /// Choose which local queues are shared. Per-user configuration.
    SelectPrinters,
    /// Start sharing the selected queues. Per-user configuration.
    StartSharing,
    /// Stop sharing. Per-user configuration.
    StopSharing,
    /// Show the certificate fingerprint clients approve. Read-only.
    ShowCertificateFingerprint,
}

impl SetupAction {
    /// Every action, in a fixed order.
    pub const ALL: [SetupAction; 5] = [
        SetupAction::AllowInboundSharing,
        SetupAction::SelectPrinters,
        SetupAction::StartSharing,
        SetupAction::StopSharing,
        SetupAction::ShowCertificateFingerprint,
    ];

    /// The stable name used by logs and by the elevated helper's command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            SetupAction::AllowInboundSharing => "allow-inbound-sharing",
            SetupAction::SelectPrinters => "select-printers",
            SetupAction::StartSharing => "start-sharing",
            SetupAction::StopSharing => "stop-sharing",
            SetupAction::ShowCertificateFingerprint => "show-certificate-fingerprint",
        }
    }

    /// Parses an action received from the command line of an elevated helper.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str() == value)
    }

    /// Whether the action changes machine-wide state and therefore needs administrator rights.
    ///
    /// Only inbound access does: the firewall rule outlives the user's session and applies to all
    /// users. Selecting queues, starting and stopping sharing, and showing the fingerprint are
    /// per-user actions and never prompt (ADR 0001).
    pub const fn requires_elevation(self) -> bool {
        matches!(self, SetupAction::AllowInboundSharing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_machine_wide_configuration_requires_elevation() {
        let elevating: Vec<&str> = SetupAction::ALL
            .into_iter()
            .filter(|action| action.requires_elevation())
            .map(SetupAction::as_str)
            .collect();

        assert_eq!(elevating, vec!["allow-inbound-sharing"]);
    }

    #[test]
    fn action_names_are_stable_and_parse_back() {
        let names: Vec<&str> = SetupAction::ALL
            .iter()
            .map(|action| action.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "allow-inbound-sharing",
                "select-printers",
                "start-sharing",
                "stop-sharing",
                "show-certificate-fingerprint",
            ]
        );
        for action in SetupAction::ALL {
            assert_eq!(SetupAction::parse(action.as_str()), Some(action));
        }
        assert_eq!(SetupAction::parse("install-printer"), None);
    }
}
