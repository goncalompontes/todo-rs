//! `todo` — a drop-in, backwards-compatible `todo.sh` host.
//!
//! Built-in commands implement the `todo.sh` core; anything else is dispatched
//! to an external action (a legacy shell/Python add-on or a Rust plugin built
//! against `todo-plugin`) discovered in the configured actions directories.
//!
//! The CLI is parsed with `clap` (global flags + built-in subcommands), while
//! `#[command(external_subcommand)]` forwards unknown actions verbatim to the
//! plugin system, preserving `todo.sh` compatibility.

mod builtins;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Parser, Subcommand};

use todo_core::config::Config;
use todo_core::error::{Error, Result};
use todo_core::style::Palette;
use todo_core::{Store, plugin};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Global options, mirroring `todo.sh`'s getopts string `:fhpcnNaAtTvVx+@Pd:`.
/// Populated from the `clap` CLI so the built-ins stay parser-agnostic.
#[derive(Debug, Clone, Default)]
pub struct Opts {
    pub force: bool,
    pub plain: Option<bool>,
    pub verbose: i32,
    pub auto_archive: Option<bool>,
    pub preserve_line_numbers: Option<bool>,
    pub date_on_add: Option<bool>,
    pub priority_on_add: Option<char>,
    pub hide_context: u32,
    pub hide_project: u32,
    pub hide_priority: bool,
    pub disable_filter: bool,
}

pub struct App {
    pub config: Config,
    pub store: Store,
    pub opts: Opts,
    pub colors: Palette,
}

/// The `clap` definition of the host CLI.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "todo",
    about = "A backwards-compatible todo.sh host with plugins",
    disable_help_flag = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
struct Cli {
    /// Configuration file to source (defaults to discovery).
    #[arg(short = 'd', long = "config", value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Force execution even when checks would normally stop it.
    #[arg(short = 'f', global = true)]
    force: bool,

    /// Plain output (no colour).
    #[arg(short = 'p', global = true)]
    plain: bool,

    /// Colour output.
    #[arg(short = 'c', global = true)]
    color: bool,

    /// Do not preserve line numbers on edit.
    #[arg(short = 'n', global = true)]
    no_line_numbers: bool,

    /// Preserve line numbers on edit.
    #[arg(short = 'N', global = true)]
    line_numbers: bool,

    /// Disable auto-archive.
    #[arg(short = 'a', global = true)]
    no_auto_archive: bool,

    /// Enable auto-archive.
    #[arg(short = 'A', global = true)]
    auto_archive: bool,

    /// Add a creation date to new tasks.
    #[arg(short = 't', global = true)]
    date_on_add: bool,

    /// Do not add a creation date to new tasks.
    #[arg(short = 'T', global = true)]
    no_date_on_add: bool,

    /// Increase verbosity.
    #[arg(short = 'v', global = true, action = ArgAction::Count)]
    verbose: u8,

    /// Print the version.
    #[arg(short = 'V', long = "version", global = true)]
    version: bool,

    /// Disable the implicit list filter.
    #[arg(short = 'x', global = true)]
    disable_filter: bool,

    /// Toggle hiding context names.
    #[arg(short = '@', global = true, action = ArgAction::Count)]
    hide_context: u8,

    /// Toggle hiding project names.
    #[arg(short = '+', global = true, action = ArgAction::Count)]
    hide_project: u8,

    /// Toggle hiding priority labels.
    #[arg(short = 'P', global = true, action = ArgAction::Count)]
    hide_priority: u8,

    /// Print help.
    #[arg(short = 'h', long = "help", global = true)]
    help: bool,

    #[command(subcommand)]
    command: Option<Action>,
}

/// Built-in `todo.sh` actions. Each captures its remaining arguments verbatim,
/// so `todo.sh` command shapes (`pri NR P`, `do NR...`, filters, ...) are kept.
#[derive(Debug, Clone, Subcommand)]
#[command(
    allow_external_subcommands = true,
    args_conflicts_with_subcommands = false
)]
enum Action {
    #[command(alias = "a")]
    Add {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Addm {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Addto {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "app")]
    Append {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Archive {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Command {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Deduplicate {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "rm")]
    Del {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "dp")]
    Depri {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "done")]
    Do {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Help {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "ls")]
    List {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "lsa")]
    Listall {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Listaddons {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "lsc")]
    Listcon {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "lf")]
    Listfile {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "lsp")]
    Listpri {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "lsprj")]
    Listproj {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "mv")]
    Move {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "prep")]
    Prepend {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "p")]
    Pri {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Replace {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Report {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Plugins {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Shorthelp {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Any action we did not define: forwarded to the plugin system.
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

impl Action {
    /// Canonical action name plus its arguments.
    fn split(self) -> (String, Vec<String>) {
        use Action::*;
        let (name, args) = match self {
            Add { args } => ("add", args),
            Addm { args } => ("addm", args),
            Addto { args } => ("addto", args),
            Append { args } => ("append", args),
            Archive { args } => ("archive", args),
            Command { args } => ("command", args),
            Deduplicate { args } => ("deduplicate", args),
            Del { args } => ("del", args),
            Depri { args } => ("depri", args),
            Do { args } => ("do", args),
            Help { args } => ("help", args),
            List { args } => ("list", args),
            Listall { args } => ("listall", args),
            Listaddons { args } => ("listaddons", args),
            Listcon { args } => ("listcon", args),
            Listfile { args } => ("listfile", args),
            Listpri { args } => ("listpri", args),
            Listproj { args } => ("listproj", args),
            Move { args } => ("move", args),
            Prepend { args } => ("prepend", args),
            Pri { args } => ("pri", args),
            Replace { args } => ("replace", args),
            Report { args } => ("report", args),
            Plugins { args } => ("plugins", args),
            Shorthelp { args } => ("shorthelp", args),
            External(values) => {
                let mut it = values.into_iter();
                let name = it
                    .next()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let args = it.map(|s| s.to_string_lossy().into_owned()).collect();
                return (name, args);
            }
        };
        (name.to_string(), args)
    }
}

fn main() -> ExitCode {
    todo_core::reset_sigpipe();
    if let Ok(exe) = std::env::current_exe() {
        // SAFETY: single-threaded at startup, before any plugin is spawned.
        unsafe {
            std::env::set_var("TODO_HOST", exe);
        }
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // clap exits for --help/--version itself only if enabled; we handle
            // errors (including our custom help/version flags) here.
            let _ = err.print();
            return ExitCode::from(2);
        }
    };

    match run(cli) {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("TODO: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<i32> {
    let opts = Opts::from_cli(&cli);

    if cli.version {
        println!("TODO.TXT Command Line Interface (todo-rs) v{VERSION}");
        return Ok(0);
    }

    let mut config = load_config(&cli.config)?;

    // A leading `-h` prints usage even with no action.
    if cli.help {
        print_usage(&config);
        return Ok(0);
    }

    // `todo.sh` supports a configured default action when called with no args.
    let command = match cli.command {
        Some(command) => command,
        None if !config.default_action.is_empty() => parse_default_action(&config.default_action)?,
        None => {
            print_usage(&config);
            return Ok(1);
        }
    };

    let plain = opts.plain.unwrap_or(config.plain);
    let (action, args) = command.split();
    let action = action.to_ascii_lowercase();

    // Splitting the action may have changed the effective config flags only
    // through env; apply the CLI overrides after we know the config.
    config.plain = plain;
    let store = Store::load(&config.file)?;
    let app = App {
        config,
        store,
        opts,
        colors: Palette::detect(plain),
    };

    // Built-ins take priority; otherwise fall through to plugins.
    if builtins::is_builtin(&action) {
        return builtins::dispatch(app, &action, &args);
    }
    if let Some(plugin) = plugin::find(&app.config, &action) {
        // The plugin needs the overrides we computed, re-exported through env.
        return plugin::run(&plugin, &app.config, &args);
    }

    eprintln!("TODO: no such action: {action}");
    eprintln!("Try 'todo help' for the list of actions.");
    Ok(1)
}

impl Opts {
    fn from_cli(cli: &Cli) -> Opts {
        let plain = if cli.plain {
            Some(true)
        } else if cli.color {
            Some(false)
        } else {
            None
        };
        Opts {
            force: cli.force,
            plain,
            verbose: 1 + cli.verbose as i32,
            auto_archive: match (cli.auto_archive, cli.no_auto_archive) {
                (true, false) => Some(true),
                (false, true) => Some(false),
                _ => None,
            },
            preserve_line_numbers: match (cli.line_numbers, cli.no_line_numbers) {
                (true, false) => Some(true),
                (false, true) => Some(false),
                _ => None,
            },
            date_on_add: match (cli.date_on_add, cli.no_date_on_add) {
                (true, false) => Some(true),
                (false, true) => Some(false),
                _ => None,
            },
            priority_on_add: None,
            hide_context: cli.hide_context as u32,
            hide_project: cli.hide_project as u32,
            hide_priority: cli.hide_priority % 2 == 1,
            disable_filter: cli.disable_filter,
        }
    }
}

fn load_config(path: &Option<PathBuf>) -> Result<Config> {
    if let Some(path) = path {
        Config::load(Some(path))
    } else {
        match Config::find_config_file() {
            Some(path) => Config::load(Some(&path)),
            None => Config::load(None),
        }
    }
}

/// Parse `TODOTXT_DEFAULT_ACTION` (a shell-ish action line) into a command.
fn parse_default_action(action: &str) -> Result<Action> {
    let mut argv: Vec<OsString> = vec![OsString::from("todo")];
    argv.extend(
        shlex::split(action)
            .unwrap_or_default()
            .into_iter()
            .map(OsString::from),
    );
    Cli::try_parse_from(argv)
        .map_err(|e| Error::usage(e.to_string()))?
        .command
        .ok_or_else(|| Error::usage("empty default action"))
}

pub fn print_usage(config: &Config) {
    println!(
        "Usage: todo [-fhpcnNaAtTvVx+@P] [-d todo_config] action [task_number] [task_description]"
    );
    println!();
    println!("Built-in actions:");
    for (line, _) in builtins::BUILTIN_HELP {
        println!("  {line}");
    }
    println!();
    let addons = plugin::discover(config);
    if !addons.is_empty() {
        println!(
            "Plugin actions (from {}):",
            config
                .actions_dirs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "  {}",
            addons
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!();
        println!("Run 'todo plugins NAME' for a plugin's usage.");
    }
    println!();
    println!("See the project README for details.");
}
