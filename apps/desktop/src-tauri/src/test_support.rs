//! Helpers the crate's own unit tests share.
//!
//! The integration tests under `tests/` cannot see this module: they link the library built without
//! `cfg(test)`, so they keep their own copies in `tests/support`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// A directory unique to one test, created and ready to write into.
pub(crate) fn temporary_directory(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("shaprint-{name}-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("creates the test directory");
    directory
}

/// A value unique to one test.
///
/// A fixture that stands in for a secret is generated rather than written out, so no repository
/// file ever contains something that reads like a real Network Channel value.
pub(crate) fn unique_value(label: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{unique}", std::process::id())
}
