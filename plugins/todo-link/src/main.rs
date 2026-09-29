//! `link` — manage clickable `todolink://` paths (OSC-8 hyperlinks).
//!
//!   t link setup [EDITOR]   register the handler; links then open EDITOR
//!   t link show             show current handler/editor/url template
//!   t link remove           unregister
//!
//! After setup, `file:`/`dir:` paths shown by `t next`, `t dep tree`, `t dep`,
//! `t ready` and `t here` become clickable in terminals that support OSC 8.
//!
//! The scheme handler itself is native: `setup` links `$cfg/link-open` at this
//! executable, which then handles `todolink://open?path=…&line=…&col=…` by
//! launching the configured editor at the recorded position.

mod editor;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use todo_core::paths;

const USAGE: &str = "  link setup [EDITOR]   register the todolink:// handler; paths become clickable\n  link show             show the current handler, editor and URL template\n  link remove           unregister the handler";

const URL_TEMPLATE: &str = "todolink://open?path={abs}&line={line}&col={col}";

fn main() {
    // The registered handler is a symlink to this binary named `link-open`.
    let invoked = todo_plugin::invoked_name();
    if invoked == "link-open" || invoked == "link_open" {
        todo_plugin::main("link-open", USAGE, |ctx| handle_url(&ctx.args));
    }
    todo_plugin::main("link", USAGE, dispatch);
}

fn dispatch(ctx: &mut todo_plugin::Context) -> todo_core::Result<i32> {
    let mut args = ctx.args.clone();
    // Legacy tolerates an explicit leading action name (`link link setup`).
    if args.first().map(String::as_str) == Some("link") {
        args.remove(0);
    }
    let sub = args.first().cloned().unwrap_or_else(|| "show".to_string());
    let rest: Vec<String> = args.get(1..).map(<[String]>::to_vec).unwrap_or_default();

    match sub.as_str() {
        "setup" => setup(&rest),
        "show" => show(),
        "remove" => remove(),
        // Extra convenience: print a clickable OSC-8 link for a path (the
        // current directory by default) so the handler can be eyeballed.
        "test" => test(&rest),
        // Hidden entry point used when invoked as `link open-url <url>`.
        "open-url" | "open_url" => handle_url(&rest),
        _ => {
            eprintln!("{USAGE}");
            Ok(1)
        }
    }
}

fn setup(rest: &[String]) -> todo_core::Result<i32> {
    let editor = rest
        .iter()
        .find(|s| !s.is_empty())
        .cloned()
        .or_else(|| nonempty("EDITOR"))
        .or_else(|| nonempty("VISUAL"))
        .or_else(|| which(&["zed", "zeditor", "nvim", "vim"]));
    let Some(editor) = editor else {
        eprintln!("TODO: no editor found; run: t link setup <editor>");
        return Ok(1);
    };

    let cfg = editor::config_dir();
    std::fs::create_dir_all(&cfg)?;
    let open_file = cfg.join("link-open");
    install_handler(&open_file, &std::env::current_exe()?)?;
    std::fs::write(cfg.join("link-editor"), &editor)?;
    std::fs::write(cfg.join("link-url"), URL_TEMPLATE)?;

    let apps = data_home().join("applications");
    std::fs::create_dir_all(&apps)?;
    let desktop = apps.join("todolink.desktop");
    let body = format!(
        "[Desktop Entry]\nType=Application\nName=Todo link opener\nExec={} %u\nMimeType=x-scheme-handler/todolink;\nNoDisplay=true\nTerminal=false\n",
        open_file.display()
    );
    std::fs::write(&desktop, body)?;

    let _ = Command::new("update-desktop-database").arg(&apps).status();
    let _ = Command::new("xdg-mime")
        .args(["default", "todolink.desktop", "x-scheme-handler/todolink"])
        .status();

    println!("TODO: file:/dir: paths now open with '{editor}' when clicked.");
    println!(
        "      (terminals with OSC 8 links: Alacritty >= 0.13, kitty, WezTerm, Ghostty, foot)"
    );
    Ok(0)
}

fn remove() -> todo_core::Result<i32> {
    let cfg = editor::config_dir();
    let apps = data_home().join("applications");
    let _ = std::fs::remove_file(apps.join("todolink.desktop"));
    let _ = std::fs::remove_file(cfg.join("link-url"));
    let _ = std::fs::remove_file(cfg.join("link-editor"));
    // Only drop `link-open` when it is the symlink we installed; leave a legacy
    // shell script in place.
    let open_file = cfg.join("link-open");
    if std::fs::symlink_metadata(&open_file)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file(&open_file);
    }
    let _ = Command::new("update-desktop-database").arg(&apps).status();
    println!("TODO: link handler removed (falls back to file:// via xdg-open).");
    Ok(0)
}

fn show() -> todo_core::Result<i32> {
    let cfg = editor::config_dir();
    let url =
        read_trimmed(cfg.join("link-url")).unwrap_or_else(|| "file://{abs} (default)".to_string());
    let editor = read_trimmed(cfg.join("link-editor")).unwrap_or_else(|| "(unset)".to_string());
    println!("url template: {url}");
    println!("editor:       {editor}");

    let handler = match Command::new("xdg-mime")
        .args(["query", "default", "x-scheme-handler/todolink"])
        .output()
    {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim_end().to_string()
        }
        _ => "(unset)".to_string(),
    };
    println!("handler:      {handler}");
    Ok(0)
}

/// Render a clickable OSC-8 link for a path using the configured template.
fn test(rest: &[String]) -> todo_core::Result<i32> {
    let target = rest.first().cloned().unwrap_or_else(|| ".".to_string());
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let reference = paths::PathRef {
        kind: paths::PathKind::Path,
        path: target,
        line: None,
        col: None,
    };
    let abs = paths::absolute(&base, &reference);
    let template = read_trimmed(editor::config_dir().join("link-url"))
        .unwrap_or_else(|| "file://{abs}".to_string());
    let url = paths::build_url(&template, &abs, None, None);

    if std::io::stdout().is_terminal() {
        println!("{}", terminal_link::Link::new(&abs.to_string_lossy(), &url));
    } else {
        println!("{url}");
    }
    Ok(0)
}

/// Native `todolink://open?path=…&line=…&col=…` handler.
fn handle_url(args: &[String]) -> todo_core::Result<i32> {
    let Some(url) = args.first() else {
        return Ok(0);
    };
    let Some(path) = url_param(url, "path").filter(|p| !p.is_empty()) else {
        return Ok(0);
    };
    let line = url_param(url, "line").and_then(|v| v.parse::<u32>().ok());
    let col = url_param(url, "col").and_then(|v| v.parse::<u32>().ok());
    let editor = editor::resolve_editor();
    editor::open_with(&editor, Path::new(&path), line, col)
}

/// Point `$cfg/link-open` at the native binary, falling back to a tiny shim.
fn install_handler(open_file: &Path, exe: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(open_file);
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(exe, open_file).is_ok() {
            return Ok(());
        }
    }
    let script = format!("#!/bin/sh\nexec {} open-url \"$@\"\n", shell_quote(exe));
    std::fs::write(open_file, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(open_file, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn url_param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(k) == key {
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(hi * 16 + lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn read_trimmed(path: PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|v| v.trim_end().to_string())
}

fn nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn data_home() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(xdg)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        PathBuf::from(".local/share")
    }
}

/// First program found on `$PATH`, mirroring `command -v`.
fn which(candidates: &[&str]) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for candidate in candidates {
        if std::env::split_paths(&path).any(|dir| is_executable(&dir.join(candidate))) {
            return Some((*candidate).to_string());
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_encoded_params() {
        let url = "todolink://open?path=%2Ftmp%2Fa%20b.rs&line=12&col=3";
        assert_eq!(url_param(url, "path").as_deref(), Some("/tmp/a b.rs"));
        assert_eq!(url_param(url, "line").as_deref(), Some("12"));
        assert_eq!(url_param(url, "col").as_deref(), Some("3"));
        assert_eq!(url_param(url, "missing"), None);
    }

    #[test]
    fn keeps_literal_plus_in_paths() {
        assert_eq!(percent_decode("a+b%2Bc"), "a+b+c");
        assert_eq!(percent_decode("%25%23"), "%#");
    }
}
