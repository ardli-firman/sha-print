//! What the shell does with its main window (#40).
//!
//! The app is meant to stay available after login and in the tray, so closing the window is not
//! quitting. The rule is kept here, free of Tauri, so it can be checked without a desktop session.

/// What closing the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Keep running: hide the window and leave the background services up.
    HideToTray,
    /// Let the window close, which ends the app and stops its services.
    Close,
}

/// The window may only hide when a tray icon exists to bring it back.
///
/// Without a tray, hiding would leave a running app the user cannot reach or quit from the window
/// they just closed.
pub const fn close_action(tray_ready: bool) -> CloseAction {
    if tray_ready {
        CloseAction::HideToTray
    } else {
        CloseAction::Close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_window_keeps_the_app_in_the_tray() {
        assert_eq!(close_action(true), CloseAction::HideToTray);
    }

    #[test]
    fn closing_the_window_without_a_tray_ends_the_app() {
        assert_eq!(close_action(false), CloseAction::Close);
    }
}
