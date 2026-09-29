//! Dependency graph: blocking, readiness, roots and `next` ranking.
//!
//! This is the reusable engine behind the `dep`/`blocked`/`ready`/`next`
//! plugins, ported from the original bash implementation.

use std::collections::{HashMap, HashSet};

use crate::date;
use crate::paths;
use crate::store::Store;
use crate::task::Task;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    Task(usize),
    Group(String),
    Missing(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Done,
    Blocked,
    Ready,
}

#[derive(Debug, Clone)]
pub struct NextEntry {
    pub index: usize,
    pub id: Option<String>,
    pub priority: Option<char>,
    pub unblocks: usize,
    pub group_clears: usize,
    pub due: Option<String>,
    pub severity: Option<u32>,
    pub score: i64,
}

pub struct DepGraph<'a> {
    /// Every task from todo.txt (open and done), in file order.
    pub all: Vec<&'a Task>,
    /// Indices into `all` for open tasks.
    pub open: Vec<usize>,
    /// First task index for each id seen anywhere (todo.txt or done.txt).
    id_state: HashMap<String, (usize, bool)>,
    /// Dependents by fine id, in file order.
    dependents_by_id: HashMap<String, Vec<usize>>,
    /// Dependents by coarse group token, in file order.
    dependents_by_group: HashMap<String, Vec<usize>>,
    /// Number of open tasks depending on each index (fine deps only).
    fine_indeg: HashMap<usize, usize>,
}

impl<'a> DepGraph<'a> {
    pub fn new(store: &'a Store, done: &'a [Task]) -> DepGraph<'a> {
        let all: Vec<&Task> = store.tasks().collect();
        let open: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.is_done())
            .map(|(i, _)| i)
            .collect();

        let mut id_state: HashMap<String, (usize, bool)> = HashMap::new();
        for (i, t) in all.iter().enumerate() {
            if let Some(id) = t.id() {
                id_state.entry(id.to_string()).or_insert((i, t.is_done()));
            }
        }
        for t in done {
            if let Some(id) = t.id() {
                // A done.txt entry means "done"; index is not in `all`.
                id_state.entry(id.to_string()).or_insert((usize::MAX, true));
            }
        }

        let mut dependents_by_id: HashMap<String, Vec<usize>> = HashMap::new();
        let mut dependents_by_group: HashMap<String, Vec<usize>> = HashMap::new();
        let mut fine_indeg: HashMap<usize, usize> = HashMap::new();
        for &i in &open {
            let t = all[i];
            for dep in t.depends() {
                if matches!(dep.chars().next(), Some('+' | '@' | '%')) {
                    continue;
                }
                dependents_by_id.entry(dep.to_string()).or_default().push(i);
                if let Some((target, _)) = id_state.get(dep)
                    && *target != usize::MAX
                {
                    *fine_indeg.entry(*target).or_insert(0) += 1;
                }
            }
            for g in t.group_depends() {
                dependents_by_group
                    .entry(g.to_string())
                    .or_default()
                    .push(i);
            }
        }

        DepGraph {
            all,
            open,
            id_state,
            dependents_by_id,
            dependents_by_group,
            fine_indeg,
        }
    }

    pub fn task(&self, i: usize) -> &Task {
        self.all[i]
    }

    pub fn is_open(&self, i: usize) -> bool {
        !self.all[i].is_done()
    }

    /// Resolved dependencies of a task, in declaration order, deduplicated.
    pub fn children(&self, i: usize) -> Vec<Child> {
        let t = self.all[i];
        let mut out = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for dep in t.depends() {
            if matches!(dep.chars().next(), Some('+' | '@' | '%')) {
                let key = format!("g:{dep}");
                if seen.insert(key) {
                    out.push(Child::Group(dep.to_string()));
                }
                continue;
            }
            let (child, key) = match self.id_state.get(dep) {
                Some((target, _)) if *target != usize::MAX => {
                    (Child::Task(*target), format!("t:{target}"))
                }
                _ => (Child::Missing(dep.to_string()), format!("m:{dep}")),
            };
            if seen.insert(key) {
                out.push(child);
            }
        }
        out
    }

    /// Human-readable blocking reasons (empty means ready).
    pub fn reasons(&self, i: usize) -> Vec<String> {
        let t = self.all[i];
        let mut out = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for dep in t.depends() {
            if matches!(dep.chars().next(), Some('+' | '@' | '%')) {
                let count = self
                    .open
                    .iter()
                    .filter(|&&j| self.all[j].has_tag(dep))
                    .count();
                if count > 0 && seen.insert(format!("grp:{dep}")) {
                    out.push(format!("depends on {dep} ({count} open task(s))"));
                }
            } else {
                match self.id_state.get(dep) {
                    Some((_, false)) if seen.insert(format!("id:{dep}")) => {
                        out.push(format!("depends on id:{dep} (open)"));
                    }
                    None if seen.insert(format!("id:{dep}")) => {
                        out.push(format!("depends on id:{dep} (MISSING)"));
                    }
                    _ => {}
                }
            }
        }

        if let Some(id) = t.id() {
            for &j in &self.open {
                if j == i {
                    continue;
                }
                if self.all[j].parents().contains(&id) {
                    // Share the `seen` set with the dependency checks above,
                    // matching the legacy `reasons_for` de-duplication.
                    let (key, label) = match self.all[j].id() {
                        Some(cid) => (format!("id:{cid}"), format!("id:{cid}")),
                        None => (
                            format!("sub:{}", self.all[j].line_no),
                            format!("line {}", self.all[j].line_no),
                        ),
                    };
                    if seen.insert(key) {
                        out.push(format!("has open subtask {label}"));
                    }
                }
            }
        }
        out
    }

    pub fn state(&self, i: usize) -> State {
        if self.all[i].is_done() {
            State::Done
        } else if self.reasons(i).is_empty() {
            State::Ready
        } else {
            State::Blocked
        }
    }

    pub fn is_ready(&self, i: usize) -> bool {
        self.state(i) == State::Ready
    }

    pub fn has_dep(&self, i: usize) -> bool {
        let t = self.all[i];
        !t.depends().is_empty()
    }

    /// Number of open tasks that (fine-)depend on `i`.
    pub fn fine_indegree(&self, i: usize) -> usize {
        self.fine_indeg.get(&i).copied().unwrap_or(0)
    }

    /// Roots: has dependencies but nothing depends on it (`indeg == 0`).
    pub fn roots(&self) -> Vec<usize> {
        self.open
            .iter()
            .copied()
            .filter(|&i| self.has_dep(i) && self.fine_indegree(i) == 0)
            .collect()
    }

    /// Tasks without dependencies and without dependents.
    pub fn independents(&self) -> Vec<usize> {
        self.open
            .iter()
            .copied()
            .filter(|&i| !self.has_dep(i) && self.fine_indegree(i) == 0)
            .collect()
    }

    /// Open tasks carrying a `+`/`@`/`%` token.
    pub fn group_members(&self, tag: &str, exclude: usize) -> Vec<usize> {
        self.open
            .iter()
            .copied()
            .filter(|&j| j != exclude && self.all[j].has_tag(tag))
            .collect()
    }

    /// Distinct open tasks transitively unblocked when task `i` is completed.
    pub fn count_unblocks(&self, i: usize) -> usize {
        let Some(id) = self.all[i].id() else {
            return 0;
        };
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut seen_tasks: HashSet<usize> = HashSet::new();
        let mut queue: Vec<String> = vec![id.to_string()];
        while let Some(cur) = queue.pop() {
            if !seen_ids.insert(cur.clone()) {
                continue;
            }
            for &x in self
                .dependents_by_id
                .get(&cur)
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if seen_tasks.insert(x)
                    && let Some(cid) = self.all[x].id()
                {
                    queue.push(cid.to_string());
                }
            }
        }
        seen_tasks.len()
    }

    /// Distinct open tasks waiting on any of the given coarse group tokens.
    pub fn group_waiting(&self, tokens: impl IntoIterator<Item = impl AsRef<str>>) -> usize {
        let mut set: HashSet<usize> = HashSet::new();
        for tok in tokens {
            if let Some(v) = self.dependents_by_group.get(tok.as_ref()) {
                set.extend(v.iter().copied());
            }
        }
        set.len()
    }

    /// Ranked ready tasks, optionally restricted to a repo-relative path.
    pub fn rank_next(
        &self,
        limit: usize,
        path_filter: Option<&str>,
        base: &std::path::Path,
    ) -> Vec<NextEntry> {
        let today = date::to_unix_days(&date::today()).unwrap_or(0);
        let mut rows: Vec<NextEntry> = Vec::new();

        for &i in &self.open {
            if !self.is_ready(i) {
                continue;
            }
            let t = self.all[i];
            if let Some(target) = path_filter
                && !paths::line_matches_path(t.tokens(), target, base)
            {
                continue;
            }
            let id = t.id().map(str::to_string);
            let priority = t.priority;
            let unblocks = if id.is_some() {
                self.count_unblocks(i)
            } else {
                0
            };
            let groups: Vec<&str> = t
                .tokens()
                .filter(|tok| matches!(tok.chars().next(), Some('+' | '@' | '%')))
                .collect();
            let group_clears = if groups.is_empty() {
                0
            } else {
                self.group_waiting(groups)
            };
            let severity = t.severity();
            let due = t.tag("due").map(str::to_string);
            let ps = match priority {
                Some('A') => 50,
                Some('B') => 30,
                Some('C') => 10,
                Some(_) => 2,
                None => 0,
            };
            let ss = match severity {
                Some(1) => 25,
                Some(2) => 10,
                _ => 0,
            };
            let db = due
                .as_deref()
                .and_then(date::to_unix_days)
                .map(|d| due_bonus(d - today))
                .unwrap_or(0);
            let score = ps + (unblocks as i64) * 25 + (group_clears as i64) * 3 + ss + db;
            rows.push(NextEntry {
                index: i,
                id,
                priority,
                unblocks,
                group_clears,
                due,
                severity,
                score,
            });
        }

        rows.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
        rows.truncate(limit);
        rows
    }
}

fn due_bonus(days: i64) -> i64 {
    if days < 0 {
        70
    } else if days <= 3 {
        50
    } else if days <= 7 {
        30
    } else if days <= 30 {
        10
    } else {
        0
    }
}
