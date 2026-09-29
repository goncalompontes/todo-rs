//! Plugin discovery and dispatch, backwards compatible with `todo.sh` actions.
//!
//! An action is either:
//!   * an executable file in one of the action directories, or
//!   * a directory containing an executable named after the directory
//!     (`actions/again/again`).
//!
//! `todo.sh` add-ons are invoked as `ACTION [args...]`, inherit the todo
//! environment, and may implement a `usage` subcommand. Rust plugins use the
//! same contract via the `todo-plugin` crate, so both kinds intermix.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

impl Action {
    /// Run the action's `usage` subcommand, returning its stdout.
    pub fn usage(&self, config: &Config) -> Option<String> {
        let out = command(self, config, &["usage".to_string()])
            .output()
            .ok()?;
        if out.stdout.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Discover actions across all configured action directories.
pub fn discover(config: &Config) -> Vec<Action> {
    let mut actions: Vec<Action> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in &config.actions_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut names: Vec<_> = entries.flatten().collect();
        names.sort_by_key(|e| e.file_name());
        for entry in names {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !seen.insert(name.clone()) {
                continue;
            }
            if executable(&path) {
                actions.push(Action {
                    name,
                    path,
                    is_dir: false,
                });
            } else if path.is_dir() {
                let inner = path.join(&name);
                if executable(&inner) {
                    actions.push(Action {
                        name,
                        path: inner,
                        is_dir: true,
                    });
                }
            }
        }
    }
    actions
}

pub fn find(config: &Config, name: &str) -> Option<Action> {
    discover(config).into_iter().find(|a| a.name == name)
}

pub fn list_names(config: &Config) -> Vec<String> {
    let mut names: Vec<String> = discover(config).into_iter().map(|a| a.name).collect();
    names.sort();
    names
}

/// Build the action's todo environment: the config env plus the canonical
/// variables, so legacy scripts see what they expect.
pub fn todo_env(config: &Config) -> Vec<(String, String)> {
    fn set(env: &mut Vec<(String, String)>, k: &str, v: String) {
        if let Some(slot) = env.iter_mut().find(|(key, _)| key == k) {
            slot.1 = v;
        } else {
            env.push((k.to_string(), v));
        }
    }

    let mut env: Vec<(String, String)> = config
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    set(
        &mut env,
        "TODO_DIR",
        config.dir.to_string_lossy().into_owned(),
    );
    set(
        &mut env,
        "TODO_FILE",
        config.file.to_string_lossy().into_owned(),
    );
    set(
        &mut env,
        "DONE_FILE",
        config.done_file.to_string_lossy().into_owned(),
    );
    set(
        &mut env,
        "REPORT_FILE",
        config.report_file.to_string_lossy().into_owned(),
    );
    if let Some(v) = &config.vocab_file {
        set(
            &mut env,
            "TODO_VOCAB_FILE",
            v.to_string_lossy().into_owned(),
        );
    }
    if let Some(dir) = config.actions_dirs.first() {
        set(
            &mut env,
            "TODO_ACTIONS_DIR",
            dir.to_string_lossy().into_owned(),
        );
    }
    if !env.iter().any(|(k, _)| k == "TODO_BASE") {
        set(
            &mut env,
            "TODO_BASE",
            config.dir.to_string_lossy().into_owned(),
        );
    }
    if let Some(cfg) = &config.config_file {
        set(&mut env, "TODO_CONFIG", cfg.to_string_lossy().into_owned());
    }
    env
}

fn command(action: &Action, config: &Config, args: &[String]) -> Command {
    let mut cmd = Command::new(&action.path);
    cmd.args(args);
    for (k, v) in todo_env(config) {
        cmd.env(k, v);
    }
    // `TODO_FULL_SH` must point at the host, not the plugin, or legacy add-ons
    // that re-invoke the CLI (`finish`, `hide`, `projectview`, ...) would
    // recurse. The host exports `TODO_HOST`; fall back to our own exe.
    let host = std::env::var("TODO_HOST")
        .ok()
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "todo".to_string());
    cmd.env("TODO_SH", "todo");
    cmd.env("TODO_FULL_SH", host);
    cmd
}

/// Run an action, returning its exit status code (never fails on non-zero).
pub fn run(action: &Action, config: &Config, args: &[String]) -> Result<i32> {
    let status = command(action, config, args).status()?;
    Ok(status.code().unwrap_or(1))
}
