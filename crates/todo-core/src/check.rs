//! Task-list validation, ported from the `check` add-on.

use std::collections::{HashMap, HashSet};

use crate::date;
use crate::store::Store;
use crate::task::Task;
use crate::vocab::Vocab;

#[derive(Debug, Clone)]
pub struct Issue {
    pub line: usize,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

pub struct Checker<'a> {
    pub store: &'a Store,
    pub done: &'a [Task],
    pub vocab: Option<&'a Vocab>,
}

impl<'a> Checker<'a> {
    pub fn run(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        self.check_vocab(&mut issues);
        self.check_dates(&mut issues);
        self.check_deps(&mut issues);
        issues
    }

    fn check_vocab(&self, issues: &mut Vec<Issue>) {
        let Some(vocab) = self.vocab else {
            return;
        };
        for task in self.store.tasks() {
            for tok in task.tokens() {
                let (axis, value) = if let Some(v) = tok.strip_prefix('+') {
                    ("module", v)
                } else if let Some(v) = tok.strip_prefix('@') {
                    ("kind", v)
                } else if let Some(v) = tok.strip_prefix("status:") {
                    ("status", v)
                } else {
                    continue;
                };
                if value.is_empty() {
                    continue;
                }
                if !vocab.allowed(axis, value) {
                    issues.push(Issue {
                        line: task.line_no,
                        severity: Severity::Warning,
                        message: format!("unknown {axis} value '{tok}'"),
                    });
                }
            }
        }
    }

    fn check_dates(&self, issues: &mut Vec<Issue>) {
        for task in self.store.tasks() {
            for key in ["due", "t"] {
                if let Some(value) = task.tag(key)
                    && date::to_unix_days(value).is_none()
                {
                    issues.push(Issue {
                        line: task.line_no,
                        severity: Severity::Error,
                        message: format!("{key}:{value} is not a YYYY-MM-DD date"),
                    });
                }
            }
        }
    }

    fn check_deps(&self, issues: &mut Vec<Issue>) {
        let mut by_id: HashMap<&str, usize> = HashMap::new();
        let mut dupes: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut known: HashSet<&str> = HashSet::new();

        for task in self.store.tasks() {
            if let Some(id) = task.id() {
                known.insert(id);
                if let Some(prev) = by_id.insert(id, task.line_no) {
                    dupes.entry(id).or_default().push(prev);
                    dupes.get_mut(id).unwrap().push(task.line_no);
                }
            }
        }
        for task in self.done {
            if let Some(id) = task.id() {
                known.insert(id);
            }
        }

        for (id, lines) in &dupes {
            for line in lines {
                issues.push(Issue {
                    line: *line,
                    severity: Severity::Error,
                    message: format!("duplicate id:{id}"),
                });
            }
        }

        for task in self.store.tasks() {
            let mut seen = HashSet::new();
            for dep in task.depends() {
                if matches!(dep.chars().next(), Some('+' | '@' | '%')) {
                    continue;
                }
                if !seen.insert(dep) {
                    continue;
                }
                if dep == task.id().unwrap_or("") {
                    issues.push(Issue {
                        line: task.line_no,
                        severity: Severity::Error,
                        message: format!("self-dependency id:{dep}"),
                    });
                } else if !known.contains(dep) {
                    issues.push(Issue {
                        line: task.line_no,
                        severity: Severity::Error,
                        message: format!("depends:{dep} refers to an unknown id"),
                    });
                }
            }
        }

        // Cycle detection over fine id dependencies.
        let deps: HashMap<&str, Vec<&str>> = self
            .store
            .tasks()
            .filter_map(|t| {
                let id = t.id()?;
                Some((
                    id,
                    t.depends()
                        .into_iter()
                        .filter(|d| !matches!(d.chars().next(), Some('+' | '@' | '%')))
                        .collect(),
                ))
            })
            .collect();

        let mut visiting: HashSet<&str> = HashSet::new();
        let mut done: HashSet<&str> = HashSet::new();
        let mut cycle: Option<Vec<String>> = None;
        for start in deps.keys() {
            if done.contains(start) {
                continue;
            }
            let mut stack: Vec<&str> = vec![start];
            visiting.insert(start);
            while let Some(&cur) = stack.last() {
                if let Some(next) = deps.get(cur)
                    && let Some(unvisited) = next.iter().find(|n| !done.contains(*n))
                {
                    if visiting.contains(unvisited) {
                        // Walk the stack from the repeated node.
                        let mut names: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
                        names.push((*unvisited).to_string());
                        cycle = Some(names);
                        break;
                    }
                    visiting.insert(unvisited);
                    stack.push(unvisited);
                    continue;
                }
                stack.pop();
                visiting.remove(cur);
                done.insert(cur);
            }
            if cycle.is_some() {
                break;
            }
        }
        if let Some(names) = cycle {
            issues.push(Issue {
                line: 0,
                severity: Severity::Error,
                message: format!("dependency cycle: {}", names.join(" -> ")),
            });
        }
    }
}
