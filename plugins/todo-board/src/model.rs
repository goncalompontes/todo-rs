//! Board data model: open tasks grouped into ordered columns by a dimension.
//!
//! This mirrors the Python `board` add-on: a *card* is an open task with its
//! blocked/ready [`State`], and *columns* are the distinct values of the chosen
//! dimension (`status`, `state`, `module`, `kind`, `group`, `priority`, `path`).

use std::collections::HashMap;

use todo_core::deps::{DepGraph, State};
use todo_core::paths::{self, PathKind};
use todo_core::task::Task;

/// The dimension cards are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Status,
    State,
    Module,
    Kind,
    Group,
    Priority,
    Path,
}

/// All dimensions, in the order `Tab` cycles through them.
pub const DIMENSIONS: [Dimension; 7] = [
    Dimension::Status,
    Dimension::State,
    Dimension::Module,
    Dimension::Kind,
    Dimension::Group,
    Dimension::Priority,
    Dimension::Path,
];

impl Dimension {
    pub fn parse(s: &str) -> Option<Dimension> {
        match s {
            "status" => Some(Dimension::Status),
            "state" => Some(Dimension::State),
            "module" => Some(Dimension::Module),
            "kind" => Some(Dimension::Kind),
            "group" => Some(Dimension::Group),
            "priority" => Some(Dimension::Priority),
            "path" => Some(Dimension::Path),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Dimension::Status => "status",
            Dimension::State => "state",
            Dimension::Module => "module (+)",
            Dimension::Kind => "kind (@)",
            Dimension::Group => "group (%)",
            Dimension::Priority => "priority",
            Dimension::Path => "path",
        }
    }

    pub fn next(self) -> Dimension {
        let i = DIMENSIONS.iter().position(|d| *d == self).unwrap_or(0);
        DIMENSIONS[(i + 1) % DIMENSIONS.len()]
    }
}

/// An open task prepared for display.
#[derive(Debug, Clone)]
pub struct Card {
    /// Index into the [`DepGraph`]'s `all` vector.
    pub index: usize,
    pub line_no: usize,
    pub id: Option<String>,
    pub priority: Option<char>,
    /// The task rendered back to its original todo.txt line.
    pub raw: String,
    pub desc: String,
    pub state: State,
    pub reasons: Vec<String>,
}

impl Card {
    /// `!` when blocked, otherwise `o` (mirrors the Python `marker`).
    pub fn marker(&self) -> &'static str {
        if self.state == State::Blocked {
            "!"
        } else {
            "o"
        }
    }

    /// One fixed-width card: `"! (A) id description"`, truncated/padded.
    pub fn card_text(&self, width: usize) -> String {
        let prio = match self.priority {
            Some(p) => format!("({p})"),
            None => "   ".to_string(),
        };
        let ident = self
            .id
            .clone()
            .unwrap_or_else(|| format!("#{}", self.line_no));
        let text = format!("{} {} {} {}", self.marker(), prio, ident, self.desc);
        fit(&text, width)
    }
}

/// An ordered column: a dimension value and the cards under it.
#[derive(Debug, Clone)]
pub struct Column {
    pub key: String,
    /// Indices into the card slice.
    pub cards: Vec<usize>,
}

/// Build a card for every open task in the graph, in file order.
pub fn cards(graph: &DepGraph<'_>) -> Vec<Card> {
    graph
        .open
        .iter()
        .map(|&i| {
            let t = graph.task(i);
            Card {
                index: i,
                line_no: t.line_no,
                id: t.id().map(str::to_string),
                priority: t.priority,
                raw: t.render(),
                desc: clean_desc(t),
                state: graph.state(i),
                reasons: graph.reasons(i),
            }
        })
        .collect()
}

/// The description shown on a card: drop `+`/`@`/`%` sigils and any
/// `key:value` tokens, exactly like the Python `clean_desc`.
pub fn clean_desc(t: &Task) -> String {
    t.tokens()
        .filter(|tok| {
            let first = tok.chars().next();
            !matches!(first, Some('+' | '@' | '%')) && !tok.contains(':')
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Column keys contributed by one task under `dim`.
pub fn column_keys(card: &Card, t: &Task, dim: Dimension) -> Vec<String> {
    match dim {
        Dimension::State => vec![state_name(card.state).to_string()],
        Dimension::Status => vec![t.tag("status").unwrap_or("(none)").to_string()],
        Dimension::Priority => vec![
            card.priority
                .map(|p| format!("({p})"))
                .unwrap_or_else(|| "(none)".to_string()),
        ],
        Dimension::Module => or_none(t.projects()),
        Dimension::Kind => or_none(t.contexts()),
        Dimension::Group => or_none(t.groups()),
        Dimension::Path => {
            let mut keys: Vec<String> = t
                .tokens()
                .filter(|tok| {
                    tok.starts_with("file:") || tok.starts_with("dir:") || tok.starts_with("path:")
                })
                .filter_map(|tok| {
                    let r = paths::parse(tok)?;
                    Some(format!("{}:{}", kind_name(r.kind), r.path))
                })
                .collect();
            if keys.is_empty() {
                keys.push("(none)".to_string());
            }
            keys
        }
    }
}

fn or_none<'a>(iter: impl Iterator<Item = &'a str>) -> Vec<String> {
    let v: Vec<String> = iter.map(str::to_string).collect();
    if v.is_empty() {
        vec!["(none)".to_string()]
    } else {
        v
    }
}

fn kind_name(kind: PathKind) -> &'static str {
    match kind {
        PathKind::File => "file",
        PathKind::Dir => "dir",
        PathKind::Path => "path",
    }
}

pub fn state_name(state: State) -> &'static str {
    match state {
        State::Done => "done",
        State::Blocked => "blocked",
        State::Ready => "ready",
    }
}

/// Group cards into ordered columns for `dim`.
pub fn build_columns(cards: &[Card], graph: &DepGraph<'_>, dim: Dimension) -> Vec<Column> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (ci, card) in cards.iter().enumerate() {
        let t = graph.task(card.index);
        for key in column_keys(card, t, dim) {
            map.entry(key).or_default().push(ci);
        }
    }

    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort_by_key(|a| sort_key(a, dim));

    keys.into_iter()
        .map(|key| {
            let mut idxs = map.remove(&key).unwrap_or_default();
            idxs.sort_by(|&a, &b| task_sort(&cards[a]).cmp(&task_sort(&cards[b])));
            Column { key, cards: idxs }
        })
        .collect()
}

/// Column ordering (mirrors the Python `sort_key`).
fn sort_key(key: &str, dim: Dimension) -> (usize, String) {
    const STATUS_ORDER: [&str; 4] = ["todo", "review", "waiting", "blocked"];
    match dim {
        Dimension::State => (if key == "blocked" { 0 } else { 1 }, key.to_string()),
        Dimension::Priority => {
            if key != "(none)" {
                (0, key.to_string())
            } else {
                (1, key.to_string())
            }
        }
        Dimension::Status => match STATUS_ORDER.iter().position(|s| *s == key) {
            Some(i) => (i, key.to_string()),
            None => (
                STATUS_ORDER.len() + usize::from(key == "(none)"),
                key.to_string(),
            ),
        },
        _ => (usize::from(key == "(none)"), key.to_lowercase()),
    }
}

/// Card ordering inside a column: priority, then blocked-before-ready, then line.
fn task_sort(card: &Card) -> (i32, i32, usize) {
    let pr = match card.priority {
        Some(p) => p.to_ascii_uppercase() as i32 - 'A' as i32,
        None => 99,
    };
    let st = if card.state == State::Blocked { 0 } else { 1 };
    (pr, st, card.line_no)
}

/// Truncate to `width` characters, padding with spaces when shorter.
pub fn fit(s: &str, width: usize) -> String {
    let mut out: String = s.chars().take(width).collect();
    let len = out.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}
