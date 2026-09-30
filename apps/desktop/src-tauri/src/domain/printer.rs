//! Local printer queues and the selection a server shares with clients.
//!
//! A queue is identified by the name the Windows spooler reports for it, which is also the name
//! clients see in an IPP `printer-name`. Nothing here touches the spooler: enumerating queues is
//! an adapter concern (`adapters::printers`).

use std::fmt;

use crate::domain::AppError;

/// Longest printer queue name the shell accepts; the Windows spooler limits names to 220
/// characters.
const NAME_LIMIT: usize = 220;

/// Name of a local Windows-managed printer queue.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrinterName(String);

impl PrinterName {
    /// Validates a queue name received from the spooler or from the UI.
    pub fn parse(value: &str) -> Result<Self, AppError> {
        let name = value.trim();
        if name.is_empty() {
            return Err(AppError::invalid_input("printer name is empty"));
        }
        if name.chars().count() > NAME_LIMIT {
            return Err(AppError::invalid_input(format!(
                "printer name is longer than {NAME_LIMIT} characters"
            )));
        }
        if name.chars().any(char::is_control) {
            return Err(AppError::invalid_input(
                "printer name contains control characters",
            ));
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PrinterName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The local queues a server user shares with clients.
///
/// Order is the order the queues were selected in, and duplicates are dropped: an IPP client sees
/// each shared printer exactly once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedPrinters(Vec<PrinterName>);

impl SharedPrinters {
    /// Builds a selection from `names`, keeping the first occurrence of each name.
    pub fn new<I: IntoIterator<Item = PrinterName>>(names: I) -> Self {
        let mut selected: Vec<PrinterName> = Vec::new();
        for name in names {
            if !selected.contains(&name) {
                selected.push(name);
            }
        }
        Self(selected)
    }

    /// Every selected queue, in selection order.
    pub fn as_slice(&self) -> &[PrinterName] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, name: &PrinterName) -> bool {
        self.0.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    fn name(value: &str) -> PrinterName {
        PrinterName::parse(value).expect("valid printer name")
    }

    #[test]
    fn names_are_trimmed_and_kept_verbatim_otherwise() {
        assert_eq!(name("  HP LaserJet  ").as_str(), "HP LaserJet");
        assert_eq!(name("HP LaserJet").to_string(), "HP LaserJet");
    }

    #[test]
    fn empty_and_control_character_names_are_rejected() {
        for invalid in ["", "   ", "HP\nLaserJet", "HP\0Jet"] {
            let error = PrinterName::parse(invalid).expect_err("rejected");
            assert_eq!(error.code(), ErrorCode::InvalidInput);
        }
    }

    #[test]
    fn overlong_names_are_rejected() {
        let error = PrinterName::parse(&"p".repeat(NAME_LIMIT + 1)).expect_err("rejected");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
        assert!(PrinterName::parse(&"p".repeat(NAME_LIMIT)).is_ok());
    }

    #[test]
    fn a_selection_drops_duplicates_and_keeps_order() {
        let selection = SharedPrinters::new([
            name("Zebra"),
            name("HP LaserJet"),
            name("Zebra"),
            name("Canon"),
        ]);

        let names: Vec<&str> = selection
            .as_slice()
            .iter()
            .map(PrinterName::as_str)
            .collect();
        assert_eq!(names, vec!["Zebra", "HP LaserJet", "Canon"]);
        assert_eq!(selection.len(), 3);
        assert!(selection.contains(&name("Canon")));
        assert!(!selection.contains(&name("Brother")));
        assert!(!SharedPrinters::default().contains(&name("Canon")));
        assert!(SharedPrinters::default().is_empty());
    }
}
