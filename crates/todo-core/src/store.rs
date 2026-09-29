//! Reading and writing todo.txt / done.txt files.

use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::error::Result;
use crate::task::Task;

/// A physical line: either a task or a blank/preserved line.
///
/// Blank lines are kept so file line numbers stay stable, which matters for
/// `todo.sh` compatibility (`todo add 12 ...` refers to a file line).
#[derive(Debug, Clone)]
pub enum Line {
    Task(Task),
    Blank(String),
}

#[derive(Debug, Clone, Default)]
pub struct Store {
    pub path: PathBuf,
    pub lines: Vec<Line>,
}

impl Store {
    pub fn load(path: &Path) -> Result<Store> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Store::from_text(path, &text))
    }

    /// Parse a store from text already in memory (used by async loaders).
    pub fn from_text(path: &Path, text: &str) -> Store {
        // `split` keeps a trailing empty element for a trailing newline; drop
        // exactly one so we don't invent an extra blank line.
        let mut raw: Vec<&str> = text.split('\n').collect();
        if raw.last() == Some(&"") {
            raw.pop();
        }
        // Parse lines in parallel; `Task::parse` is pure and each line is
        // independent.
        let lines: Vec<Line> = raw
            .par_iter()
            .enumerate()
            .map(|(i, line)| {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if line.trim().is_empty() {
                    Line::Blank(line.to_string())
                } else {
                    Line::Task(Task::parse(i + 1, line))
                }
            })
            .collect();
        Store {
            path: path.to_path_buf(),
            lines,
        }
    }

    /// All tasks in file order (skipping blank lines).
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.lines.iter().filter_map(|l| match l {
            Line::Task(t) => Some(t),
            Line::Blank(_) => None,
        })
    }

    pub fn tasks_mut(&mut self) -> impl Iterator<Item = &mut Task> {
        self.lines.iter_mut().filter_map(|l| match l {
            Line::Task(t) => Some(t),
            Line::Blank(_) => None,
        })
    }

    pub fn open_tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks().filter(|t| !t.is_done())
    }

    pub fn task_by_line(&self, n: usize) -> Option<&Task> {
        match self.lines.get(n.checked_sub(1)?)? {
            Line::Task(t) => Some(t),
            Line::Blank(_) => None,
        }
    }

    pub fn task_by_line_mut(&mut self, n: usize) -> Option<&mut Task> {
        match self.lines.get_mut(n.checked_sub(1)?)? {
            Line::Task(t) => Some(t),
            Line::Blank(_) => None,
        }
    }

    pub fn task_by_id(&self, id: &str) -> Option<&Task> {
        self.tasks().find(|t| t.id() == Some(id))
    }

    /// Find the (first) open task matching a selector: a line number or a
    /// stable `id:` slug.
    pub fn find(&self, selector: &str) -> Option<&Task> {
        if let Ok(n) = selector.parse::<usize>() {
            return self.task_by_line(n);
        }
        self.open_tasks()
            .find(|t| t.id() == Some(selector))
            .or_else(|| self.task_by_id(selector))
    }

    /// The next free 1-based line number (one past the last physical line).
    pub fn next_line_no(&self) -> usize {
        self.lines.len() + 1
    }

    /// Insert a rendered line at 1-based `line_no`, shifting the rest down.
    pub fn insert_line(&mut self, line_no: usize, text: String) {
        let idx = line_no.saturating_sub(1).min(self.lines.len());
        let line = if text.trim().is_empty() {
            Line::Blank(text)
        } else {
            Line::Task(Task::parse(0, &text))
        };
        self.lines.insert(idx, line);
        self.reindex();
    }

    pub fn remove_line(&mut self, line_no: usize) {
        if line_no >= 1 && line_no <= self.lines.len() {
            self.lines.remove(line_no - 1);
            self.reindex();
        }
    }

    pub fn replace_line(&mut self, line_no: usize, text: String) {
        if line_no >= 1 && line_no <= self.lines.len() {
            let line = if text.trim().is_empty() {
                Line::Blank(text)
            } else {
                Line::Task(Task::parse(line_no, &text))
            };
            self.lines[line_no - 1] = line;
        }
    }

    /// Recompute every task's `line_no` after a structural edit.
    pub fn reindex(&mut self) {
        for (i, line) in self.lines.iter_mut().enumerate() {
            if let Line::Task(t) = line {
                t.line_no = i + 1;
            }
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Task(t) => out.push_str(&t.render()),
                Line::Blank(s) => out.push_str(s),
            }
            out.push('\n');
        }
        out
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, self.render())?;
        Ok(())
    }
}
