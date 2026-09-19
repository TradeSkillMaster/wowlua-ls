//! Returning freed heap memory to the OS after heavy parallel phases.
//!
//! The allocator caches freed pages per thread and purges them only as that thread
//! keeps allocating. The rayon workers behind the startup scan and the workspace warm
//! go idle as soon as they finish, so their scratch memory would otherwise stay
//! resident for the life of the server (~0.65 GB on wow-ui-source).

use std::sync::OnceLock;

static RELEASE_HOOK: OnceLock<fn()> = OnceLock::new();

/// Register the allocator's "release this thread's free memory" function. The binary
/// owns the global allocator, so it supplies the hook; without one, releases are no-ops.
pub fn set_release_memory_hook(hook: fn()) {
    let _ = RELEASE_HOOK.set(hook);
}

/// Run the release hook on every thread of `pool` (rayon's global pool when `None`)
/// and on the calling thread.
pub(super) fn release_memory(pool: Option<&rayon::ThreadPool>) {
    let Some(&hook) = RELEASE_HOOK.get() else { return };
    match pool {
        Some(pool) => {
            pool.broadcast(|_| hook());
        }
        None => {
            rayon::broadcast(|_| hook());
        }
    }
    hook();
}
