//! Shared library for the `todo` toolchain: a dependency-free todo.txt engine
//! with configuration loading, a task model, a dependency graph, validation and
//! `todo.sh`-compatible plugin dispatch.

pub mod check;
pub mod config;
pub mod date;
pub mod deps;
pub mod diagnostics;
pub mod error;
pub mod format;
pub mod parse;
pub mod paths;
pub mod plugin;
pub mod store;
pub mod style;
pub mod task;
pub mod validate;
pub mod vocab;

pub use config::Config;
pub use error::{Error, Result};
pub use store::{Line, Store};
pub use task::Task;

/// Restore the default `SIGPIPE` behaviour so piping into `head`/`less` exits
/// quietly instead of panicking inside `println!`.
#[cfg(unix)]
pub fn reset_sigpipe() {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    // SIGPIPE = 13, SIG_DFL = 0 on Linux.
    unsafe {
        signal(13, 0);
    }
}

#[cfg(not(unix))]
pub fn reset_sigpipe() {}
