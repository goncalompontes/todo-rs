//! `open` — open the file or directory a task pertains to.
//!
//!   t open N          open task N's file:/dir:/path: location in $EDITOR
//!   t open N --print  just print the resolved path
//!
//! Paths come from the `file:`/`dir:`/`path:` metadata token on the task.
//! When stdout is not a terminal the path is printed instead of launching an
//! editor, matching the `open` add-on.

mod editor;

use std::io::IsTerminal;
use std::path::Path;

use todo_core::paths;

const USAGE: &str =
    "  open N [--print]\n    Open task N's file:/dir: path in $EDITOR (or print it with --print).";

fn main() {
    // A symlink named `editor-open` turns this binary into the editor launcher
    // itself (`editor-open PATH [LINE] [COL]`), as the legacy chain expects.
    let invoked = todo_plugin::invoked_name();
    if invoked == "editor-open" || invoked == "editor_open" {
        todo_plugin::main("editor-open", USAGE, |ctx| launch(&ctx.args));
    }
    todo_plugin::main("open", USAGE, run);
}

fn run(ctx: &mut todo_plugin::Context) -> todo_core::Result<i32> {
    let mut args: Vec<&str> = ctx.args.iter().map(String::as_str).collect();
    // Legacy tolerates an explicit leading action name (`open open N`).
    if args.first() == Some(&"open") {
        args.remove(0);
    }

    let mut print_only = false;
    let mut selector: Option<&str> = None;
    for arg in args {
        match arg {
            "--print" => print_only = true,
            other => selector = Some(other),
        }
    }

    let Some(selector) = selector else {
        eprintln!("TODO: usage: t open N [--print]");
        return Ok(1);
    };

    let Ok(n) = selector.parse::<usize>() else {
        eprintln!("TODO: no task #{selector}");
        return Ok(1);
    };
    let Some(task) = ctx.store.task_by_line(n) else {
        eprintln!("TODO: no task #{selector}");
        return Ok(1);
    };
    let Some(token) = paths::first_path_token(task.tokens()) else {
        eprintln!("TODO: task #{selector} has no file:/dir: path");
        return Ok(1);
    };
    let Some(reference) = paths::parse(&token) else {
        eprintln!("TODO: task #{selector} has no file:/dir: path");
        return Ok(1);
    };

    let base = ctx
        .config
        .file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ctx.config.dir.clone());
    let abs = paths::absolute(&base, &reference);
    if !abs.exists() {
        eprintln!("TODO: path does not exist: {}", reference.path);
        return Ok(1);
    }

    if print_only || !std::io::stdout().is_terminal() {
        println!("{}", format_location(&abs, reference.line, reference.col));
        return Ok(0);
    }

    let editor = editor::resolve_editor();
    editor::open_with(&editor, &abs, reference.line, reference.col)
}

/// `editor-open PATH [LINE] [COL]` entry point (when invoked by that name).
fn launch(args: &[String]) -> todo_core::Result<i32> {
    let Some(path) = args.first() else {
        return Ok(1);
    };
    let line = args.get(1).and_then(|v| v.parse::<u32>().ok());
    let col = args.get(2).and_then(|v| v.parse::<u32>().ok());
    let editor = editor::resolve_editor();
    editor::open_with(&editor, Path::new(path), line, col)
}

fn format_location(path: &Path, line: Option<u32>, col: Option<u32>) -> String {
    let mut out = path.display().to_string();
    if let Some(line) = line {
        out.push_str(&format!(":{line}"));
    }
    if let Some(col) = col {
        out.push_str(&format!(":{col}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_line_and_col() {
        let path = Path::new("/x/y.rs");
        assert_eq!(format_location(path, Some(4), Some(2)), "/x/y.rs:4:2");
        assert_eq!(format_location(path, Some(4), None), "/x/y.rs:4");
        assert_eq!(format_location(path, None, None), "/x/y.rs");
    }
}
