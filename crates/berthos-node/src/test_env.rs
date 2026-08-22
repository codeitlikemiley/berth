//! One process-wide lock for tests that touch the environment.
//!
//! `setenv`/`unsetenv` are not thread-safe -- the reason Rust 2024 made them
//! `unsafe`. libc may reallocate the `environ` array, and anything reading it
//! concurrently can see a torn or freed pointer. `Command::spawn` is such a
//! reader: it walks the whole environment to build `envp`.
//!
//! Per-module locks are not enough. Two modules each serialising against
//! themselves still let one mutate the environment while the other spawns a
//! process, which surfaces as a child that simply cannot see a variable the
//! test just set. Every test in this crate that mutates or depends on the
//! environment takes *this* lock, so there is exactly one.

use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Hold this for as long as the environment is being read *or* mutated --
/// including across any `Command::spawn` that must observe the change.
///
/// Poisoning is ignored on purpose: a test that panicked mid-mutation has
/// already failed, and refusing the lock afterwards would cascade that single
/// failure across every other environment test in the run.
pub(crate) fn lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
