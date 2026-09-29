//! SDK for native Rust plugins.
//!
//! A plugin is an ordinary executable following the `todo.sh` add-on contract
//! (inherits `TODO_FILE` & friends, receives its arguments, implements
//! `usage`). Two entry points are provided:
//!
//! * [`main`] — closure style, for quick one-off actions.
//! * [`run_plugin`] + [`Plugin`] — trait style, for larger plugins.
//!
//! ```no_run
//! use todo_plugin::prelude::*;
//!
//! struct Hello;
//! impl Plugin for Hello {
//!     fn name(&self) -> &str { "hello" }
//!     fn usage(&self) -> &str { "  hello\n    Say hello." }
//!     fn run(&self, ctx: &mut Context) -> Result<i32> {
//!         println!("{} open tasks", ctx.store.open_tasks().count());
//!         Ok(0)
//!     }
//! }
//! fn main() { todo_plugin::run_plugin(Hello); }
//! ```

pub mod args;

use std::path::Path;

use todo_core::check::Checker;
use todo_core::config::Config;
use todo_core::deps::DepGraph;
use todo_core::error::{Error, Result};
use todo_core::store::Store;
use todo_core::style::Palette;
use todo_core::task::Task;
use todo_core::vocab::Vocab;

/// Everything a plugin needs: config, task data, arguments and shared style.
pub struct Context {
    pub config: Config,
    pub store: Store,
    pub done: Vec<Task>,
    pub vocab: Option<Vocab>,
    pub args: Vec<String>,
    pub colors: Palette,
}

impl Context {
    pub fn load() -> Result<Context> {
        let cfg = std::env::var_os("TODO_CONFIG").map(std::path::PathBuf::from);
        let config = Config::load(cfg.as_deref()).or_else(|_| Config::load(None))?;
        let store = Store::load(&config.file)?;
        let done = load_done(&config);
        let vocab = config
            .vocab_file
            .as_deref()
            .and_then(|p| Vocab::load(p).ok());
        let colors = Palette::detect(config.plain);
        Ok(Context {
            config,
            store,
            done,
            vocab,
            args: Vec::new(),
            colors,
        })
    }

    pub fn graph(&self) -> DepGraph<'_> {
        DepGraph::new(&self.store, &self.done)
    }

    pub fn vocab(&self) -> Option<&Vocab> {
        self.vocab.as_ref()
    }

    pub fn checker(&self) -> Checker<'_> {
        Checker {
            store: &self.store,
            done: &self.done,
            vocab: self.vocab.as_ref(),
        }
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn print_error(&self, err: &Error) {
        eprintln!("TODO: {err}");
    }
}

fn load_done(config: &Config) -> Vec<Task> {
    Store::load(&config.done_file)
        .map(|s| s.tasks().cloned().collect())
        .unwrap_or_default()
}

/// Trait for structured plugins.
pub trait Plugin {
    fn name(&self) -> &str;
    fn usage(&self) -> &str;
    fn run(&self, ctx: &mut Context) -> Result<i32>;
}

/// Run a [`Plugin`], handling `usage`/`help`/`-h` and exit codes.
pub fn run_plugin<P: Plugin>(plugin: P) -> ! {
    let name = plugin.name().to_string();
    let usage = plugin.usage().to_string();
    main(&name, &usage, move |ctx| plugin.run(ctx))
}

/// Basename of `argv[0]`, so one binary can serve several action names.
pub fn invoked_name() -> String {
    std::env::args()
        .next()
        .as_deref()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "todo-plugin".to_string())
}

/// Standard plugin entry point. Prints `usage` for `usage`/`help`/`-h`, loads
/// the context, invokes `f`, and exits with the returned status.
pub fn main<F>(name: &str, usage: &str, f: F) -> !
where
    F: FnOnce(&mut Context) -> Result<i32>,
{
    todo_core::reset_sigpipe();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        raw.first().map(String::as_str),
        Some("usage" | "help" | "-h" | "--help")
    ) {
        println!("{usage}");
        std::process::exit(0);
    }

    let mut ctx = match Context::load() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{name}: {e}");
            std::process::exit(2);
        }
    };
    ctx.args = raw;

    let code = match f(&mut ctx) {
        Ok(code) => code,
        Err(Error::External(code)) => code,
        Err(e) => {
            ctx.print_error(&e);
            1
        }
    };
    std::process::exit(code);
}

/// Re-exports for a one-line `use todo_plugin::prelude::*;` in plugins.
pub mod prelude {
    pub use crate::args::Args;
    pub use crate::{Context, Plugin, main, run_plugin};
    pub use todo_core::deps::{Child, DepGraph, State};
    pub use todo_core::error::{Error, Result};
    pub use todo_core::style::{self, Palette};
    pub use todo_core::task::Task;
    pub use todo_core::{Config, Store};
}
