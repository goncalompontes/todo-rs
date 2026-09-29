//! `todo` — a drop-in, backwards-compatible `todo.sh` host.
//!
//! Built-in commands implement the `todo.sh` core; anything else is dispatched
//! to an external action (a legacy shell/Python add-on or a Rust plugin built
//! against `todo-plugin`) discovered in the configured actions directories.

mod builtins;

use std::path::PathBuf;
use std::process::ExitCode;

use todo_core::config::Config;
use todo_core::error::{Error, Result};
use todo_core::style::Palette;
use todo_core::{Store, plugin};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Global options, mirroring `todo.sh`'s getopts string `:fhpcnNaAtTvVx+@Pd:`.
#[derive(Debug, Clone, Default)]
pub struct Opts {
    pub config: Option<PathBuf>,
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
    pub help: bool,
    pub version: bool,
}

pub struct App {
    pub config: Config,
    pub store: Store,
    pub opts: Opts,
    pub colors: Palette,
}

fn main() -> ExitCode {
    todo_core::reset_sigpipe();
    if let Ok(exe) = std::env::current_exe() {
        // SAFETY: single-threaded at startup, before any plugin is spawned.
        unsafe {
            std::env::set_var("TODO_HOST", exe);
        }
    }
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(argv) {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("TODO: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(argv: Vec<String>) -> Result<i32> {
    let (opts, action, args) = parse_args(&argv)?;

    if opts.version {
        println!("TODO.TXT Command Line Interface (todo-rs) v{VERSION}");
        return Ok(0);
    }

    let config = load_config(&opts)?;

    if opts.help {
        print_usage(&config);
        return Ok(0);
    }

    let plain = opts.plain.unwrap_or(config.plain);
    let store = Store::load(&config.file)?;
    let app = App {
        config,
        store,
        opts,
        colors: Palette::detect(plain),
    };

    let action = action.unwrap_or_else(|| app.config.default_action.clone());
    if action.is_empty() {
        print_usage(&app.config);
        return Ok(1);
    }
    let action = action.to_ascii_lowercase();

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

fn load_config(opts: &Opts) -> Result<Config> {
    let mut config = if let Some(path) = &opts.config {
        Config::load(Some(path))?
    } else {
        match Config::find_config_file() {
            Some(path) => Config::load(Some(&path))?,
            None => Config::load(None)?,
        }
    };
    if let Some(v) = opts.auto_archive {
        config.auto_archive = v;
    }
    if let Some(v) = opts.preserve_line_numbers {
        config.preserve_line_numbers = v;
    }
    if let Some(v) = opts.date_on_add {
        config.date_on_add = v;
    }
    if let Some(p) = opts.priority_on_add {
        config.priority_on_add = Some(p);
    }
    if let Some(v) = opts.plain {
        config.plain = v;
    }
    config.hide_context = opts.hide_context;
    config.hide_project = opts.hide_project;
    Ok(config)
}

/// Parse the global options and the action/args, `todo.sh`-style.
fn parse_args(argv: &[String]) -> Result<(Opts, Option<String>, Vec<String>)> {
    let mut opts = Opts {
        verbose: 1,
        ..Opts::default()
    };
    let mut i = 0;
    let mut action = None;
    let mut args = Vec::new();
    let mut after_double_dash = false;

    while i < argv.len() {
        let arg = &argv[i];
        if after_double_dash || action.is_some() {
            if action.is_none() {
                action = Some(arg.clone());
            } else {
                args.push(arg.clone());
            }
            i += 1;
            continue;
        }
        if arg == "--" {
            after_double_dash = true;
            i += 1;
            continue;
        }
        if arg == "-" || !arg.starts_with('-') {
            action = Some(arg.clone());
            i += 1;
            continue;
        }

        // Long options we support for convenience.
        match arg.as_str() {
            "--help" => {
                opts.help = true;
                i += 1;
                continue;
            }
            "--version" => {
                opts.version = true;
                i += 1;
                continue;
            }
            "--plain" => {
                opts.plain = Some(true);
                i += 1;
                continue;
            }
            "--color" => {
                opts.plain = Some(false);
                i += 1;
                continue;
            }
            _ => {}
        }

        let mut chars = arg[1..].chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                'h' => opts.help = true,
                'f' => opts.force = true,
                'p' => opts.plain = Some(true),
                'c' => opts.plain = Some(false),
                'n' => opts.preserve_line_numbers = Some(false),
                'N' => opts.preserve_line_numbers = Some(true),
                'a' => opts.auto_archive = Some(false),
                'A' => opts.auto_archive = Some(true),
                't' => opts.date_on_add = Some(true),
                'T' => opts.date_on_add = Some(false),
                'v' => opts.verbose += 1,
                'V' => opts.version = true,
                'x' => opts.disable_filter = true,
                '+' => opts.hide_project = opts.hide_project.wrapping_add(1),
                '@' => opts.hide_context = opts.hide_context.wrapping_add(1),
                'P' => opts.hide_priority = !opts.hide_priority,
                'd' => {
                    let rest: String = chars.collect();
                    let value = if rest.is_empty() {
                        i += 1;
                        argv.get(i)
                            .cloned()
                            .ok_or_else(|| Error::usage("option -d requires an argument"))?
                    } else {
                        rest
                    };
                    opts.config = Some(PathBuf::from(value));
                    break;
                }
                other => {
                    return Err(Error::usage(format!("unknown option -{other}")));
                }
            }
        }
        i += 1;
    }

    Ok((opts, action, args))
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
