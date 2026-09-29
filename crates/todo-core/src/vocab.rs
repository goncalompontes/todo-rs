//! `.todo/todo.vocab` parsing and validation.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct Vocab {
    /// axis -> allowed values, in file order.
    pub axes: BTreeMap<String, Vec<String>>,
}

impl Vocab {
    pub fn load(path: &Path) -> Result<Vocab> {
        let text = fs::read_to_string(path)?;
        Ok(Vocab::parse(&text))
    }

    pub fn parse(text: &str) -> Vocab {
        let mut axes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((axis, values)) = line.split_once(':') else {
                continue;
            };
            let list: Vec<String> = values.split_whitespace().map(str::to_string).collect();
            axes.insert(axis.trim().to_string(), list);
        }
        Vocab { axes }
    }

    pub fn allowed(&self, axis: &str, value: &str) -> bool {
        self.axes
            .get(axis)
            .map(|v| v.iter().any(|x| x == value))
            .unwrap_or(true)
    }

    pub fn values(&self, axis: &str) -> Option<&[String]> {
        self.axes.get(axis).map(Vec::as_slice)
    }

    pub fn add(&mut self, axis: &str, values: &[String]) {
        let entry = self.axes.entry(axis.to_string()).or_default();
        for v in values {
            if !entry.contains(v) {
                entry.push(v.clone());
            }
        }
        entry.sort();
    }

    /// Render back to the vocab file format.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# todo.vocab - allowed values for this repo's todo.txt tags.\n");
        out.push_str(
            "#\n# Format: one axis per line, `axis: value value ...`; `#` starts a comment.\n\n",
        );
        for (axis, values) in &self.axes {
            out.push_str(axis);
            out.push(':');
            for v in values {
                out.push(' ');
                out.push_str(v);
            }
            out.push('\n');
        }
        out
    }
}
