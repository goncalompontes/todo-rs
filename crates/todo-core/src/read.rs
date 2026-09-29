//! A zero-copy, read-only view over a todo.txt source buffer.
//!
//! `Document<'a>` borrows the file text and yields `DocTask<'a>` values whose
//! tokens, tags and description borrow from that same buffer. It reuses the
//! span-accurate `chumsky` parser, so no text is copied. Use the owned
//! [`crate::Store`] when you need to mutate and save.

use crate::parse::{self, ParsedLine};
use crate::task::TaskJson;

#[derive(Debug, Clone)]
pub struct DocTask<'a> {
    pub line_no: usize,
    pub raw: &'a str,
    pub parsed: ParsedLine<'a>,
}

impl<'a> DocTask<'a> {
    pub fn is_done(&self) -> bool {
        self.parsed.done
    }

    pub fn id(&self) -> Option<&'a str> {
        self.parsed.id()
    }

    pub fn tag(&self, key: &str) -> Option<&'a str> {
        self.parsed.tag(key)
    }

    pub fn depends(&self) -> Vec<&'a str> {
        self.parsed.depends()
    }

    pub fn tokens(&self) -> impl Iterator<Item = &'a str> {
        self.parsed.tokens.iter().map(|t| t.text)
    }

    fn sigils(&self, sigil: char) -> impl Iterator<Item = &'a str> {
        self.tokens().filter(move |t| t.starts_with(sigil))
    }

    pub fn projects(&self) -> impl Iterator<Item = &'a str> {
        self.sigils('+')
    }

    pub fn contexts(&self) -> impl Iterator<Item = &'a str> {
        self.sigils('@')
    }

    pub fn groups(&self) -> impl Iterator<Item = &'a str> {
        self.sigils('%')
    }

    /// The description slice, borrowed from the raw line.
    pub fn description(&self) -> &'a str {
        match self.parsed.tokens.first() {
            Some(first) => self.raw[first.span.start..].trim_end(),
            None => "",
        }
    }

    pub fn path(&self) -> Option<&'a str> {
        self.tokens()
            .find(|t| t.starts_with("file:") || t.starts_with("dir:") || t.starts_with("path:"))
    }

    pub fn severity(&self) -> Option<u32> {
        self.tag("sev").and_then(|v| v.parse().ok())
    }

    pub fn to_json(&self) -> TaskJson<'a> {
        TaskJson {
            line: self.line_no,
            done: self.is_done(),
            priority: self.parsed.priority,
            completion_date: self.parsed.completion_date,
            creation_date: self.parsed.creation_date,
            description: self.description(),
            id: self.id(),
            projects: self.projects().collect(),
            contexts: self.contexts().collect(),
            groups: self.groups().collect(),
            depends: self.depends(),
            severity: self.severity(),
            due: self.tag("due"),
            path: self.path(),
        }
    }
}

/// A parsed, borrowed todo.txt file.
#[derive(Debug, Clone)]
pub struct Document<'a> {
    pub source: &'a str,
    pub tasks: Vec<DocTask<'a>>,
}

impl<'a> Document<'a> {
    pub fn parse(source: &'a str) -> Document<'a> {
        let mut tasks = Vec::new();
        for (i, raw) in source.split_inclusive('\n').enumerate() {
            let line = raw.strip_suffix('\n').unwrap_or(raw);
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.trim().is_empty() {
                continue;
            }
            if let Some(parsed) = parse::parse_line(line, 0).line {
                tasks.push(DocTask {
                    line_no: i + 1,
                    raw: line,
                    parsed,
                });
            }
        }
        Document { source, tasks }
    }

    pub fn open_tasks(&self) -> impl Iterator<Item = &DocTask<'a>> {
        self.tasks.iter().filter(|t| !t.is_done())
    }

    pub fn task_by_line(&self, n: usize) -> Option<&DocTask<'a>> {
        self.tasks.iter().find(|t| t.line_no == n)
    }

    /// Resolve a line number or `id:` selector.
    pub fn find(&self, selector: &str) -> Option<&DocTask<'a>> {
        if let Ok(n) = selector.parse::<usize>() {
            return self.task_by_line(n);
        }
        self.tasks.iter().find(|t| t.id() == Some(selector))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrows_tokens_and_description() {
        let src = "(A) 2026-09-01 buy milk +shop @errand id:x depends:y\nplain +core\n";
        let doc = Document::parse(src);
        assert_eq!(doc.tasks.len(), 2);

        let t = doc.find("x").unwrap();
        assert_eq!(t.description(), "buy milk +shop @errand id:x depends:y");
        assert_eq!(t.id(), Some("x"));
        assert_eq!(t.depends(), vec!["y"]);
        assert_eq!(t.projects().collect::<Vec<_>>(), vec!["+shop"]);
        assert_eq!(t.contexts().collect::<Vec<_>>(), vec!["@errand"]);
        assert_eq!(t.path(), None);

        assert!(doc.find("1").is_some());
        assert_eq!(doc.open_tasks().count(), 2);
    }

    #[test]
    fn json_view_is_borrowed() {
        let src = "task +core\n";
        let doc = Document::parse(src);
        let json = doc.tasks[0].to_json();
        assert_eq!(json.projects, vec!["+core"]);
        assert_eq!(json.description, "task +core");
    }
}
