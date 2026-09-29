//! A single todo.txt line.

use crate::date;

/// A parsed task, keeping enough structure to re-render the original line.
///
/// Only the prefix (`x`, priority, dates) is parsed; the description is kept
/// verbatim so unknown tokens survive a round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub line_no: usize,
    pub done: bool,
    pub priority: Option<char>,
    /// Completion date, only meaningful for done tasks.
    pub completion_date: Option<String>,
    /// Creation date.
    pub creation_date: Option<String>,
    pub description: String,
}

impl Task {
    pub fn parse(line_no: usize, raw: &str) -> Task {
        let raw = raw.trim_end_matches(['\n', '\r']);
        let mut rest = raw;

        let done = if let Some(stripped) = rest.strip_prefix("x ") {
            rest = stripped;
            true
        } else if rest == "x" {
            return Task {
                line_no,
                done: true,
                priority: None,
                completion_date: None,
                creation_date: None,
                description: String::new(),
            };
        } else {
            false
        };

        let mut priority = None;
        if let Some(letter) = parse_priority(rest) {
            priority = Some(letter);
            rest = &rest[4..];
        }

        let mut dates = Vec::new();
        while let Some((date, tail)) = date::parse_prefix(rest) {
            // A date must be followed by a space or end of line.
            if !tail.is_empty() && !tail.starts_with(' ') {
                break;
            }
            dates.push(date);
            rest = tail.trim_start();
            if dates.len() == 2 {
                break;
            }
        }

        let (completion_date, creation_date) = match (done, dates.len()) {
            (true, 2) => (Some(dates[0].clone()), Some(dates[1].clone())),
            (true, 1) => (Some(dates[0].clone()), None),
            (false, 1) => (None, Some(dates[0].clone())),
            _ => (None, None),
        };

        Task {
            line_no,
            done,
            priority,
            completion_date,
            creation_date,
            description: rest.to_string(),
        }
    }

    pub fn new(line_no: usize, description: impl Into<String>) -> Task {
        Task {
            line_no,
            done: false,
            priority: None,
            completion_date: None,
            creation_date: None,
            description: description.into(),
        }
    }

    /// Render back to a todo.txt line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.done {
            out.push_str("x ");
        }
        if let Some(p) = self.priority {
            out.push('(');
            out.push(p);
            out.push_str(") ");
        }
        if let Some(d) = &self.completion_date {
            out.push_str(d);
            out.push(' ');
        }
        if let Some(d) = &self.creation_date {
            out.push_str(d);
            out.push(' ');
        }
        out.push_str(&self.description);
        out
    }

    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        self.description.split_whitespace()
    }

    pub fn projects(&self) -> impl Iterator<Item = &str> {
        self.tokens().filter(|t| t.starts_with('+') && t.len() > 1)
    }

    pub fn contexts(&self) -> impl Iterator<Item = &str> {
        self.tokens().filter(|t| t.starts_with('@') && t.len() > 1)
    }

    pub fn groups(&self) -> impl Iterator<Item = &str> {
        self.tokens().filter(|t| t.starts_with('%') && t.len() > 1)
    }

    /// Value of the first `key:value` token, if any (`key` without the colon).
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tokens().find_map(|t| {
            let (k, v) = t.split_once(':')?;
            (k == key).then_some(v)
        })
    }

    pub fn has_tag(&self, token: &str) -> bool {
        self.tokens().any(|t| t == token)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn id(&self) -> Option<&str> {
        self.tag("id").filter(|v| !v.is_empty())
    }

    /// Fine-grained dependency targets (`depends:`/`dep:`), comma-split.
    pub fn depends(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for t in self.tokens() {
            if let Some(v) = t
                .strip_prefix("depends:")
                .or_else(|| t.strip_prefix("dep:"))
            {
                out.extend(v.split(',').filter(|s| !s.is_empty()));
            }
        }
        out
    }

    /// Topydo parent links (`p:`), comma-split.
    pub fn parents(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for t in self.tokens() {
            if let Some(v) = t.strip_prefix("p:") {
                out.extend(v.split(',').filter(|s| !s.is_empty()));
            }
        }
        out
    }

    /// Coarse dependency tags (`depends:+project`, `depends:@context`,
    /// `depends:%group`).
    pub fn group_depends(&self) -> Vec<&str> {
        self.tokens()
            .filter_map(|t| {
                t.strip_prefix("depends:")
                    .or_else(|| t.strip_prefix("dep:"))
            })
            .filter(|v| matches!(v.chars().next(), Some('+' | '@' | '%')))
            .collect()
    }

    /// Severity from `sev:N`.
    pub fn severity(&self) -> Option<u32> {
        self.tag("sev")?.parse().ok()
    }

    // --- mutation ---------------------------------------------------------

    pub fn set_priority(&mut self, priority: Option<char>) {
        self.priority = priority;
    }

    pub fn mark_done(&mut self, completion: String) {
        if !self.done {
            self.done = true;
            self.completion_date = Some(completion);
        }
        self.priority = None;
    }

    pub fn set_description(&mut self, description: impl Into<String>) {
        self.description = description.into();
    }

    /// Add a whitespace token to the description if absent.
    pub fn add_desc_token(&mut self, token: &str) {
        if !self.has_tag_exact(token) {
            self.append(token);
        }
    }

    fn has_tag_exact(&self, token: &str) -> bool {
        self.tokens().any(|t| t == token)
    }

    /// Remove every exact token from the description.
    pub fn remove_desc_token(&mut self, token: &str) {
        let kept: Vec<&str> = self.tokens().filter(|t| *t != token).collect();
        self.description = kept.join(" ");
    }

    /// Remove every token starting with `prefix`.
    pub fn remove_tokens_with_prefix(&mut self, prefix: &str) {
        let kept: Vec<&str> = self.tokens().filter(|t| !t.starts_with(prefix)).collect();
        self.description = kept.join(" ");
    }

    /// Replace the first `key:value` token (any value) with a new one.
    pub fn set_tag(&mut self, key: &str, value: &str) {
        let mut replaced = false;
        let mut out: Vec<String> = Vec::new();
        for t in self.tokens() {
            if !replaced && t.split_once(':').map(|(k, _)| k == key).unwrap_or(false) {
                out.push(format!("{key}:{value}"));
                replaced = true;
            } else {
                out.push(t.to_string());
            }
        }
        if !replaced {
            out.push(format!("{key}:{value}"));
        }
        self.description = out.join(" ");
    }

    pub fn remove_tag(&mut self, key: &str) {
        let kept: Vec<&str> = self
            .tokens()
            .filter(|t| t.split_once(':').map(|(k, _)| k != key).unwrap_or(true))
            .collect();
        self.description = kept.join(" ");
    }

    pub fn append(&mut self, text: &str) {
        if !self.description.is_empty() {
            self.description.push(' ');
        }
        self.description.push_str(text);
    }

    pub fn prepend(&mut self, text: &str) {
        if self.description.is_empty() {
            self.description.push_str(text);
        } else {
            self.description = format!("{text} {}", self.description);
        }
    }
}

/// `(A) text` -> `Some('A')`. The caller has already stripped `x `.
fn parse_priority(s: &str) -> Option<char> {
    let bytes = s.as_bytes();
    if bytes.len() >= 4 && bytes[0] == b'(' && bytes[2] == b')' && bytes[3] == b' ' {
        let c = bytes[1] as char;
        if c.is_ascii_uppercase() {
            return Some(c);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active_with_priority_and_date() {
        let t = Task::parse(1, "(A) 2026-09-29 buy milk +shop @errand");
        assert_eq!(t.priority, Some('A'));
        assert_eq!(t.creation_date.as_deref(), Some("2026-09-29"));
        assert_eq!(t.description, "buy milk +shop @errand");
        assert!(!t.done);
    }

    #[test]
    fn parses_done_with_two_dates() {
        let t = Task::parse(2, "x 2026-09-29 2026-09-01 ship it");
        assert!(t.done);
        assert_eq!(t.completion_date.as_deref(), Some("2026-09-29"));
        assert_eq!(t.creation_date.as_deref(), Some("2026-09-01"));
        assert_eq!(t.description, "ship it");
    }

    #[test]
    fn round_trips() {
        for line in [
            "buy milk",
            "(B) buy milk +shop",
            "x 2026-09-29 2026-09-01 ship it",
            "x 2026-09-29 done without creation",
        ] {
            assert_eq!(Task::parse(1, line).render(), line);
        }
    }

    #[test]
    fn extracts_tags() {
        let t = Task::parse(1, "task +mod @kind id:x depends:a,b p:c sev:2");
        assert_eq!(t.id(), Some("x"));
        assert_eq!(t.depends(), vec!["a", "b"]);
        assert_eq!(t.parents(), vec!["c"]);
        assert_eq!(t.severity(), Some(2));
    }
}
