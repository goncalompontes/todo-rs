//! Editor resolution and launching, ported from the `editor-open` add-on.
//!
//! `editor-open PATH [LINE] [COL]` picks an editor and encodes the position in
//! the way that editor expects: `code -g`, `zed path:line:col`, `nano +line,col`,
//! `vi +call cursor(line,col)`, and so on. The editor command is word-split so
//! values like `code --wait` keep working.

use std::path::{Path, PathBuf};
use std::process::Command;

use todo_core::Result;

/// `$XDG_CONFIG_HOME/todo`, falling back to `$HOME/.config/todo`.
pub fn config_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(xdg).join("todo")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config/todo")
    } else {
        PathBuf::from(".config/todo")
    }
}

fn nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Resolve the editor command.
///
/// Precedence: `$TODO_EDITOR`, `$EDITOR`, `$VISUAL`, the configured
/// `link-editor` file, then `vi`. `editor-open` read `link-editor` first; the
/// port keeps `$TODO_EDITOR`/`$EDITOR` as the interactive overrides and lets
/// the configured editor (what the desktop handler normally sees) fall back.
pub fn resolve_editor() -> String {
    if let Some(editor) = nonempty("TODO_EDITOR") {
        return editor;
    }
    if let Some(editor) = nonempty("EDITOR") {
        return editor;
    }
    if let Some(editor) = nonempty("VISUAL") {
        return editor;
    }
    if let Some(editor) = std::fs::read_to_string(config_dir().join("link-editor"))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return editor;
    }
    "vi".to_string()
}

/// Launch `editor` on `path` at `line`/`col`, returning the editor's exit code.
pub fn open_with(editor: &str, path: &Path, line: Option<u32>, col: Option<u32>) -> Result<i32> {
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let mut cmd = Command::new(program);
    cmd.args(parts);

    let path = path.to_string_lossy();
    let base = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    let pos = match (line, col) {
        (Some(l), Some(c)) => Some(format!("{l}:{c}")),
        (Some(l), None) => Some(l.to_string()),
        _ => None,
    };

    match base {
        "code" | "codium" | "cursor" | "code-oss" => {
            if let Some(pos) = pos {
                cmd.arg("-g").arg(format!("{path}:{pos}"));
            } else {
                cmd.arg(path.as_ref());
            }
        }
        "zed" | "zeditor" => {
            if let Some(pos) = pos {
                cmd.arg(format!("{path}:{pos}"));
            } else {
                cmd.arg(path.as_ref());
            }
        }
        "nano" => {
            if let (Some(l), Some(c)) = (line, col) {
                cmd.arg(format!("+{l},{c}"));
            } else if let Some(l) = line {
                cmd.arg(format!("+{l}"));
            }
            cmd.arg(path.as_ref());
        }
        "emacs" => {
            if let Some(pos) = pos {
                cmd.arg(format!("+{pos}"));
            }
            cmd.arg(path.as_ref());
        }
        "vi" | "vim" | "nvim" | "hx" | "helix" | "kak" => {
            if let (Some(l), Some(c)) = (line, col) {
                cmd.arg(format!("+call cursor({l},{c})"));
            } else if let Some(l) = line {
                cmd.arg(format!("+{l}"));
            }
            cmd.arg(path.as_ref());
        }
        _ => {
            if let Some(l) = line {
                cmd.arg(format!("+{l}"));
            }
            cmd.arg(path.as_ref());
        }
    }

    let status = cmd.status()?;
    Ok(status.code().unwrap_or(1))
}
