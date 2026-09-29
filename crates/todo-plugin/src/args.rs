//! A small, consistent argument parser for plugins.
//!
//! Supports long flags (`--verbose`), long values (`--width 80` or
//! `--width=80`), bare short flags (`-v`), `--` to end option parsing, and
//! positionals. This keeps every plugin's CLI handling uniform.

use std::collections::{HashMap, HashSet};

use todo_core::error::{Error, Result};

#[derive(Debug, Default, Clone)]
pub struct Args {
    flags: HashSet<String>,
    values: HashMap<String, String>,
    rest: Vec<String>,
}

impl Args {
    /// Parse `argv` given the set of boolean `flags` and value `options`.
    pub fn parse(argv: &[String], flags: &[&str], options: &[&str]) -> Result<Args> {
        let mut args = Args::default();
        let mut i = 0;
        let mut only_rest = false;
        while i < argv.len() {
            let arg = &argv[i];
            if only_rest {
                args.rest.push(arg.clone());
                i += 1;
                continue;
            }
            if arg == "--" {
                only_rest = true;
                i += 1;
                continue;
            }
            if let Some(body) = arg.strip_prefix("--") {
                if body.is_empty() {
                    args.rest.push(arg.clone());
                } else if let Some((k, v)) = body.split_once('=') {
                    if options.contains(&k) {
                        args.values.insert(k.to_string(), v.to_string());
                    } else if flags.contains(&k) {
                        return Err(Error::usage(format!("--{k} takes no value")));
                    } else {
                        return Err(Error::usage(format!("unknown option --{k}")));
                    }
                } else if flags.contains(&body) {
                    args.flags.insert(body.to_string());
                } else if options.contains(&body) {
                    i += 1;
                    let v = argv
                        .get(i)
                        .ok_or_else(|| Error::usage(format!("--{body} requires a value")))?;
                    args.values.insert(body.to_string(), v.clone());
                } else {
                    return Err(Error::usage(format!("unknown option --{body}")));
                }
            } else if let Some(body) = arg.strip_prefix('-') {
                if body.is_empty() {
                    args.rest.push(arg.clone());
                } else if flags.contains(&body) {
                    args.flags.insert(body.to_string());
                } else if options.contains(&body) {
                    i += 1;
                    let v = argv
                        .get(i)
                        .ok_or_else(|| Error::usage(format!("-{body} requires a value")))?;
                    args.values.insert(body.to_string(), v.clone());
                } else {
                    return Err(Error::usage(format!("unknown option -{body}")));
                }
            } else {
                args.rest.push(arg.clone());
            }
            i += 1;
        }
        Ok(args)
    }

    pub fn has(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn value<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        match self.values.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<T>()
                .map(Some)
                .map_err(|e| Error::usage(format!("--{key}: {e}"))),
        }
    }

    pub fn rest(&self) -> &[String] {
        &self.rest
    }
}
