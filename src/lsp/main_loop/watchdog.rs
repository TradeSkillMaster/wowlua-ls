//! Main-loop watchdog: a background thread that reports when the single-threaded
//! request loop has been blocked on one operation for too long.
//!
//! `main_loop` processes notifications, document (re)analysis, and requests
//! inline on a single thread (see the Phase 1–4 comments in `mod.rs`). A single
//! pathological analysis or query therefore stalls *every* pending request — the
//! client reports a wall of "no response from the server" timeouts with no
//! indication of the culprit (exactly the failure mode this module exists to
//! diagnose). The watchdog records the label and start time of the work item the
//! main thread is currently executing; a detached thread polls that state and,
//! once an item exceeds a threshold, logs a warning naming the stuck operation
//! and the file/method it concerns, so the next freeze is self-diagnosing from
//! the server log alone.
//!
//! This is observability only: it never touches analysis state, so it has no
//! effect on diagnostics, determinism, or results.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The work item the main loop is currently executing, or `None` when idle.
/// Set/cleared by [`WorkGuard`] around each blocking main-thread call and read by
/// the watchdog thread. The lock is only ever held briefly to set, clear, or
/// snapshot the value (never across the guarded work itself), so the watchdog
/// never contends with the main thread while it is stalled.
static CURRENT_WORK: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// RAII marker recording that the main thread has begun a unit of blocking work.
/// Bind one to a live local (`let _wg = WorkGuard::new(...)`) immediately before a
/// call that runs inline on the main loop; the current work is cleared when it
/// drops. Guards are created only at `main_loop` call sites and never nest (one
/// item runs at a time on the single request thread).
pub(super) struct WorkGuard;

impl WorkGuard {
    pub(super) fn new(label: impl Into<String>) -> Self {
        if let Ok(mut cur) = CURRENT_WORK.lock() {
            *cur = Some((label.into(), Instant::now()));
        }
        WorkGuard
    }
}

impl Drop for WorkGuard {
    fn drop(&mut self) {
        if let Ok(mut cur) = CURRENT_WORK.lock() {
            *cur = None;
        }
    }
}

/// Best-effort `textDocument.uri` extraction from a request/notification's
/// params, used to label which file an operation concerns.
pub(super) fn message_uri(params: &serde_json::Value) -> Option<&str> {
    params.get("textDocument")?.get("uri")?.as_str()
}

/// Spawn the detached watchdog thread. Call once at server start.
pub(super) fn spawn_watchdog() {
    // Poll interval and the stall threshold past which we start warning. A
    // healthy interactive analysis/request completes in well under a second, so
    // a multi-second block already signals trouble; we then re-log on a coarse
    // cadence so a true hang leaves an escalating trail ("blocked for 5s… 10s…
    // 60s…") rather than a single line.
    const POLL: Duration = Duration::from_secs(2);
    const THRESHOLD: Duration = Duration::from_secs(5);
    const RELOG_STEP_SECS: u64 = 5;

    let spawned = std::thread::Builder::new()
        .name("wowlua-watchdog".to_string())
        .spawn(|| {
            // (started, last elapsed-seconds logged) for the op currently being
            // warned about: re-log on the coarse cadence, and reset when a new
            // op begins or the loop goes idle.
            let mut logged_for: Option<(Instant, u64)> = None;
            loop {
                std::thread::sleep(POLL);
                let snapshot = CURRENT_WORK.lock().ok().and_then(|cur| cur.clone());
                match snapshot {
                    Some((label, started)) => {
                        let elapsed = started.elapsed();
                        if elapsed >= THRESHOLD {
                            let secs = elapsed.as_secs();
                            let should_log = match logged_for {
                                Some((s, last)) if s == started => {
                                    secs >= last + RELOG_STEP_SECS
                                }
                                _ => true,
                            };
                            if should_log {
                                log::warn!(
                                    "main loop blocked for {secs}s on: {label} — \
                                     the single-threaded request loop is stalled, \
                                     so all pending requests will time out"
                                );
                                logged_for = Some((started, secs));
                            }
                        } else {
                            logged_for = None;
                        }
                    }
                    None => logged_for = None,
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("failed to spawn watchdog thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_uri_extracts_uri() {
        let params = json!({"textDocument": {"uri": "file:///foo.lua"}});
        assert_eq!(message_uri(&params), Some("file:///foo.lua"));
    }

    #[test]
    fn message_uri_missing_text_document() {
        assert_eq!(message_uri(&json!({})), None);
        assert_eq!(message_uri(&json!({"position": {"line": 0}})), None);
    }

    #[test]
    fn message_uri_missing_uri_key() {
        let params = json!({"textDocument": {"version": 1}});
        assert_eq!(message_uri(&params), None);
    }

    #[test]
    fn message_uri_uri_not_string() {
        let params = json!({"textDocument": {"uri": 42}});
        assert_eq!(message_uri(&params), None);
    }

    #[test]
    fn work_guard_sets_and_clears() {
        {
            let _wg = WorkGuard::new("test op");
            let snap = CURRENT_WORK.lock().unwrap();
            assert_eq!(snap.as_ref().unwrap().0, "test op");
        }
        let snap = CURRENT_WORK.lock().unwrap();
        assert!(snap.is_none());
    }
}
