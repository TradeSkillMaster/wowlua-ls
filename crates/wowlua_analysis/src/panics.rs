//! Helpers for containing panics: what a caught panic said, and (in test builds)
//! a way to cause one.
//!
//! The language server catches a panic while it analyzes one file or answers one
//! request, so the panic costs that file's results or that response rather than
//! the server; a cross-file harvest catches one in the file it reads
//! (`deferred::analyze_file`). Stack overflow aborts the process and can't be caught.

use std::any::Any;

/// The message a caught panic's payload carries: what `panic!`, `expect`, an
/// out-of-bounds index, etc. said.
pub fn message(payload: &(dyn Any + Send)) -> String {
    match payload.downcast_ref::<&str>() {
        Some(s) => (*s).to_string(),
        None => payload.downcast_ref::<String>().cloned().unwrap_or_else(|| "unknown panic".to_string()),
    }
}

/// Test-only fault injection: panic when `text` contains `--@panic:<site>`, so
/// tests can show a panic at `site` is contained. Compiled out of every other build.
#[cfg(any(test, feature = "test-util"))]
pub fn inject(site: &str, text: &str) {
    if text.contains(&format!("--@panic:{site}")) {
        panic!("injected panic at {site}");
    }
}

/// Test-only fault injection; does nothing outside test builds.
#[cfg(not(any(test, feature = "test-util")))]
#[inline(always)]
pub fn inject(_site: &str, _text: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_reads_str_and_string_payloads() {
        let caught = std::panic::catch_unwind(|| panic!("plain")).unwrap_err();
        assert_eq!(message(&*caught), "plain");
        let caught = std::panic::catch_unwind(|| panic!("formatted {}", 7)).unwrap_err();
        assert_eq!(message(&*caught), "formatted 7");
        let caught = std::panic::catch_unwind(|| std::panic::panic_any(42_u8)).unwrap_err();
        assert_eq!(message(&*caught), "unknown panic");
    }

    #[test]
    fn inject_panics_only_on_its_own_marker() {
        inject("analysis", "local x = 1 --@panic:scan");
        let caught = std::panic::catch_unwind(|| inject("analysis", "--@panic:analysis")).unwrap_err();
        assert_eq!(message(&*caught), "injected panic at analysis");
    }
}
