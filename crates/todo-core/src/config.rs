//! Configuration discovery and loading.
//!
//! `todo.sh` config files are bash scripts (`export TODO_FILE=...`), sometimes
//! with command substitutions, so for full backwards compatibility we source
//! the file through `/bin/bash` and capture the resulting environment. A
//! dependency-free fallback parser handles simple configs when bash is not
//! available.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub config_file: Option<PathBuf>,
    pub dir: PathBuf,
    pub file: PathBuf,
    pub done_file: PathBuf,
    pub report_file: PathBuf,
    pub vocab_file: Option<PathBuf>,
    pub actions_dirs: Vec<PathBuf>,
    pub default_action: String,
    pub auto_archive: bool,
    pub preserve_line_numbers: bool,
    pub plain: bool,
    pub date_on_add: bool,
    pub priority_on_add: Option<char>,
    pub hide_context: u32,
    pub hide_project: u32,
    pub final_filter: Option<String>,
    pub sort_command: Option<String>,
    pub verbose: bool,
    pub base: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl Config {
    /// Load configuration.
    ///
    /// When `config_file` is given it is sourced (bash) or parsed (fallback);
    /// otherwise the current process environment is used as-is.
    pub fn load(config_file: Option<&Path>) -> Result<Config> {
        let env = match config_file {
            Some(path) => {
                if !path.exists() {
                    return Err(Error::config(format!(
                        "configuration file not found: {}",
                        path.display()
                    )));
                }
                source_env(path)
            }
            None => std::env::vars().collect(),
        };
        Ok(Config::from_env(config_file.map(Path::to_path_buf), env))
    }

    /// Discover a config file the way `todo.sh` does, returning the first that
    /// exists. `TODO_DIR`/`$PWD` lookups mirror the common `t` shell function.
    pub fn find_config_file() -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("TODO_CONFIG") {
            candidates.push(PathBuf::from(p));
        }
        if let Ok(dir) = std::env::var("TODO_DIR") {
            candidates.push(Path::new(&dir).join(".todo/config"));
        }
        if let Ok(dir) = std::env::var("PWD") {
            candidates.push(Path::new(&dir).join(".todo/config"));
        }
        if let Ok(p) = std::env::current_dir() {
            candidates.push(p.join(".todo/config"));
        }
        if let Some(home) = home_dir() {
            candidates.push(home.join(".todo/config"));
        }
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            candidates.push(Path::new(&xdg).join("todo/config"));
        } else if let Some(home) = home_dir() {
            candidates.push(home.join(".config/todo/config"));
        }
        candidates.push(PathBuf::from("/etc/todo/config"));
        candidates.into_iter().find(|p| p.is_file())
    }

    fn from_env(config_file: Option<PathBuf>, env: BTreeMap<String, String>) -> Config {
        let get = |k: &str| env.get(k).cloned().unwrap_or_default();
        let truthy = |k: &str| is_truthy(env.get(k).map(String::as_str).unwrap_or(""));

        let dir = env
            .get("TODO_DIR")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let file = env
            .get("TODO_FILE")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| dir.join("todo.txt"));

        let done_file = env
            .get("DONE_FILE")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| dir.join("done.txt"));

        let report_file = env
            .get("REPORT_FILE")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| dir.join("report.txt"));

        let vocab_file = env
            .get("TODO_VOCAB_FILE")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                let p = dir.join(".todo/todo.vocab");
                p.is_file().then_some(p)
            });

        let mut config = Config {
            config_file,
            base: dir.clone(),
            dir,
            file,
            done_file,
            report_file,
            vocab_file,
            actions_dirs: actions_dirs(&env),
            default_action: env
                .get("TODOTXT_DEFAULT_ACTION")
                .cloned()
                .unwrap_or_default(),
            auto_archive: env
                .get("TODOTXT_AUTO_ARCHIVE")
                .map(|v| is_truthy(v))
                .unwrap_or(true),
            preserve_line_numbers: env
                .get("TODOTXT_PRESERVE_LINE_NUMBERS")
                .map(|v| is_truthy(v))
                .unwrap_or(true),
            plain: truthy("TODOTXT_PLAIN"),
            date_on_add: truthy("TODOTXT_DATE_ON_ADD"),
            priority_on_add: get("TODOTXT_PRIORITY_ON_ADD")
                .chars()
                .next()
                .filter(|c| c.is_ascii_uppercase()),
            hide_context: 0,
            hide_project: 0,
            final_filter: env.get("TODOTXT_FINAL_FILTER").cloned(),
            sort_command: env.get("TODOTXT_SORT_COMMAND").cloned(),
            verbose: truthy("TODOTXT_VERBOSE"),
            env: env.clone(),
        };
        config.dir = expand(config.dir);
        config.file = expand(config.file);
        config.done_file = expand(config.done_file);
        config.report_file = expand(config.report_file);
        config.vocab_file = config.vocab_file.map(expand);
        config
    }

    pub fn read_file(&self) -> Result<Vec<crate::task::Task>> {
        crate::store::Store::load(&self.file).map(|s| s.tasks().cloned().collect())
    }
}

/// Directories searched for external actions, in priority order.
fn actions_dirs(env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |p: PathBuf| {
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    };
    // Native Rust plugins take precedence over legacy add-ons with the same
    // name, so a ported action shadows its shell/Python predecessor.
    if let Some(p) = env.get("TODO_RUST_ACTIONS_DIR").filter(|p| !p.is_empty()) {
        push(PathBuf::from(p));
    }
    if let Some(home) = home_dir() {
        push(home.join(".config/todo/rust-actions"));
    }
    if let Some(p) = env.get("TODO_ACTIONS_DIR").filter(|p| !p.is_empty()) {
        push(PathBuf::from(p));
    }
    if let Some(home) = home_dir() {
        push(home.join(".todo/actions"));
        push(home.join(".config/todo/actions"));
        push(home.join(".todo.actions.d"));
    }
    dirs.into_iter().filter(|p| p.is_dir()).collect()
}

pub fn is_truthy(s: &str) -> bool {
    !matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Expand a leading `~` in a configured path.
fn expand(path: PathBuf) -> PathBuf {
    PathBuf::from(shellexpand::tilde(&path.to_string_lossy()).into_owned())
}

/// Source a bash config file and return the resulting environment.
fn source_env(path: &Path) -> BTreeMap<String, String> {
    // `$0` is set to "todo-config" so configs using `${BASH_SOURCE[0]:-$0}`
    // and `dirname` resolve relative to the file after `.`.
    let script = r#"set -a
. "$1" >/dev/null 2>&1
exec env -0"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .arg("todo-config")
        .arg(path)
        .output();

    match output {
        Ok(out) if out.status.success() => parse_nul_env(&out.stdout),
        _ => parse_shell_config(path),
    }
}

fn parse_nul_env(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for entry in bytes.split(|b| *b == 0) {
        if entry.is_empty() {
            continue;
        }
        if let Ok(s) = std::str::from_utf8(entry)
            && let Some((k, v)) = s.split_once('=')
        {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

/// Minimal fallback: `export K=V`, `K=V`, `unset K`, `# comments`.
fn parse_shell_config(path: &Path) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> = std::env::vars().collect();
    let Ok(text) = std::fs::read_to_string(path) else {
        return map;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some(name) = line.strip_prefix("unset ") {
            map.remove(name.trim());
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(v);
        let v = v
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .unwrap_or(v);
        map.insert(k.to_string(), v.to_string());
    }
    map
}
