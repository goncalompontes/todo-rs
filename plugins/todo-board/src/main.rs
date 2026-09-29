//! `board` — a multi-panel todo.txt board (GitHub Projects style).
//!
//! Columns are the values of a chosen dimension (status, state, module, kind,
//! group, priority, path); cards are open tasks showing priority, id,
//! description and blocked state.
//!
//! Interactive (ratatui):
//!     t board [--by status|state|module|kind|group|priority|path]
//! Non-interactive (plain text, for piping/screenshots):
//!     t board --snapshot [--by status] [--no-color] [--width N]
//!
//! Keys: q quit · r reload · h/l or <-/-> column · j/k or ^/v card
//!       g/G top/bottom · 1-6 or Tab switch dimension · enter details

mod model;
mod snapshot;
mod tui;

use std::io::IsTerminal;
use std::path::PathBuf;

use todo_core::error::Error;
use todo_core::store::Store;
use todo_core::style::Palette;
use todo_core::task::Task;
use todo_plugin::Context;

use model::Dimension;

const USAGE: &str = "  board [--by status|state|module|kind|group|priority|path]\n        [--snapshot] [--no-color] [--width N]";

fn main() {
    todo_plugin::main("board", USAGE, run);
}

fn run(ctx: &mut Context) -> todo_core::Result<i32> {
    let mut args = ctx.args.clone();
    // Legacy dispatch sometimes passes the action name as the first argument.
    if args.first().map(String::as_str) == Some("board") {
        args.remove(0);
    }
    let opts = parse_args(&args)?;

    let stdout_tty = std::io::stdout().is_terminal();
    if opts.snapshot || !stdout_tty {
        let width = if opts.width == 0 {
            term_width()
        } else {
            opts.width
        };
        // Guard against overflow in the column-width arithmetic for absurd
        // values (the visible result is identical for anything this large).
        let width = width.clamp(0, 1_000_000);
        let graph = ctx.graph();
        let cards = model::cards(&graph);
        let columns = model::build_columns(&cards, &graph, opts.dim);
        let colors = Palette::new(snapshot_color(opts.no_color, ctx.config.plain));
        print!("{}", snapshot::render(&cards, &columns, width, colors));
        Ok(0)
    } else {
        let mut app = App::new(
            ctx.store.clone(),
            ctx.done.clone(),
            ctx.config.file.clone(),
            ctx.config.done_file.clone(),
            opts.dim,
        );
        tui::run(&mut app)?;
        Ok(0)
    }
}

/// Interactive board state.
struct App {
    store: Store,
    done: Vec<Task>,
    todo_path: PathBuf,
    done_path: PathBuf,
    dim: Dimension,
    col: usize,
    row: usize,
    /// Index of the first visible column.
    coff: usize,
    detail: bool,
}

impl App {
    fn new(
        store: Store,
        done: Vec<Task>,
        todo_path: PathBuf,
        done_path: PathBuf,
        dim: Dimension,
    ) -> App {
        App {
            store,
            done,
            todo_path,
            done_path,
            dim,
            col: 0,
            row: 0,
            coff: 0,
            detail: false,
        }
    }

    /// Re-read `todo.txt`/`done.txt` from disk, like the legacy `r` key.
    fn reload(&mut self) -> todo_core::Result<()> {
        self.store = Store::load(&self.todo_path)?;
        self.done = Store::load(&self.done_path)
            .map(|s| s.tasks().cloned().collect())
            .unwrap_or_default();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct Options {
    dim: Dimension,
    snapshot: bool,
    no_color: bool,
    /// 0 means "use the terminal width".
    width: i64,
}

fn parse_args(args: &[String]) -> todo_core::Result<Options> {
    let mut opts = Options {
        dim: Dimension::Status,
        snapshot: false,
        no_color: false,
        width: 0,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--snapshot" {
            opts.snapshot = true;
        } else if arg == "--no-color" {
            opts.no_color = true;
        } else if let Some(value) = arg.strip_prefix("--by=") {
            opts.dim = parse_dim(value)?;
        } else if arg == "--by" {
            i += 1;
            let value = args
                .get(i)
                .ok_or_else(|| Error::usage("board: --by requires a value"))?;
            opts.dim = parse_dim(value)?;
        } else if let Some(value) = arg.strip_prefix("--width=") {
            opts.width = parse_width(value)?;
        } else if arg == "--width" {
            i += 1;
            let value = args
                .get(i)
                .ok_or_else(|| Error::usage("board: --width requires a value"))?;
            opts.width = parse_width(value)?;
        } else {
            return Err(Error::usage(format!(
                "board: unrecognized argument '{arg}'"
            )));
        }
        i += 1;
    }
    Ok(opts)
}

fn parse_dim(value: &str) -> todo_core::Result<Dimension> {
    Dimension::parse(value).ok_or_else(|| {
        Error::usage(format!(
            "board: invalid --by value '{value}' (choose status|state|module|kind|group|priority|path)"
        ))
    })
}

fn parse_width(value: &str) -> todo_core::Result<i64> {
    value
        .trim()
        .parse::<i64>()
        .map_err(|_| Error::usage(format!("board: invalid --width value '{value}'")))
}

/// Snapshot colours follow the legacy rule (a terminal, unless `--no-color`)
/// plus `NO_COLOR` and the todo config's `plain` setting.
fn snapshot_color(no_color: bool, plain: bool) -> bool {
    if no_color || plain {
        return false;
    }
    if std::env::var_os("NO_COLOR")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// Terminal width, then `$COLUMNS`, then 120 — as the legacy board does.
fn term_width() -> i64 {
    if let Some(w) = crossterm::terminal::size()
        .ok()
        .map(|(w, _)| w)
        .filter(|w| *w > 0)
    {
        return w as i64;
    }
    if let Some(n) = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|n| *n > 0)
    {
        return n;
    }
    120
}
