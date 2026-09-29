//! Terminal formatting shared by the host and plugins.

use regex::RegexBuilder;

use crate::config::Config;
use crate::store::Store;
use crate::style::{self, Palette};
use crate::task::Task;

/// Format a single task for list output.
pub fn format_task(
    task: &Task,
    palette: Palette,
    hide_context: bool,
    hide_project: bool,
) -> String {
    let mut out = String::new();

    if task.done {
        out.push_str(&palette.paint(style::dim(), "x"));
        out.push(' ');
    }
    if let Some(p) = task.priority {
        out.push_str(&palette.paint(style::priority(p), format!("({p})")));
        out.push(' ');
    }
    if let Some(d) = &task.completion_date {
        out.push_str(d);
        out.push(' ');
    }
    if let Some(d) = &task.creation_date {
        out.push_str(d);
        out.push(' ');
    }

    let mut first = true;
    for tok in task.tokens() {
        if !first {
            out.push(' ');
        }
        first = false;
        let rendered = if tok.starts_with('+') {
            if hide_project {
                String::new()
            } else {
                palette.paint(style::sigil(tok), tok)
            }
        } else if tok.starts_with('@') {
            if hide_context {
                String::new()
            } else {
                palette.paint(style::sigil(tok), tok)
            }
        } else if tok.starts_with('%') {
            palette.paint(style::sigil(tok), tok)
        } else {
            tok.to_string()
        };
        out.push_str(&rendered);
    }

    if task.done {
        palette.paint(style::dim(), &out)
    } else {
        out
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    pub all: bool,
    pub hide_context: bool,
    pub hide_project: bool,
    /// Treat `filters` as case-insensitive regular expressions.
    pub regex: bool,
    pub filters: Vec<String>,
}

impl ListOptions {
    pub fn from_config(config: &Config) -> ListOptions {
        ListOptions {
            all: false,
            hide_context: config.hide_context % 2 == 1,
            hide_project: config.hide_project % 2 == 1,
            regex: false,
            filters: Vec::new(),
        }
    }
}

/// Validate regex filters up-front so listing can't silently match nothing.
pub fn validate_filters(filters: &[String], regex: bool) -> crate::error::Result<()> {
    if !regex {
        return Ok(());
    }
    for f in filters {
        RegexBuilder::new(f)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::Error::usage(format!("invalid regex '{f}': {e}")))?;
    }
    Ok(())
}

fn matches_filters(task: &Task, filters: &[String], regex: bool) -> bool {
    if filters.is_empty() {
        return true;
    }
    if regex {
        let hay = task.render();
        filters.iter().all(|f| {
            RegexBuilder::new(f)
                .case_insensitive(true)
                .build()
                .map(|re| re.is_match(&hay))
                .unwrap_or(false)
        })
    } else {
        let hay = task.render().to_ascii_lowercase();
        filters
            .iter()
            .all(|f| hay.contains(&f.to_ascii_lowercase()))
    }
}

/// The tasks a [`ListOptions`] selects (used by text and JSON output alike).
pub fn select_tasks<'a>(store: &'a Store, opts: &ListOptions) -> Vec<&'a Task> {
    store
        .tasks()
        .filter(|t| opts.all || !t.is_done())
        .filter(|t| matches_filters(t, &opts.filters, opts.regex))
        .collect()
}

/// Render a filtered list, right-aligning line numbers like `todo.sh list`.
pub fn render_list(store: &Store, opts: &ListOptions, palette: Palette) -> String {
    let selected = select_tasks(store, opts);
    if selected.is_empty() {
        return format!(
            "{}\n",
            palette.paint(style::dim(), "TODO: no tasks to list")
        );
    }
    let width = selected
        .iter()
        .map(|t| t.line_no.to_string().len())
        .max()
        .unwrap_or(1);
    let mut out = String::new();
    for task in selected {
        let text = format_task(task, palette, opts.hide_context, opts.hide_project);
        out.push_str(&format!(
            "{:>width$} {}\n",
            task.line_no,
            text,
            width = width
        ));
    }
    out
}

/// Truncate to `width` display columns, appending `…` when cut.
pub fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        return s.to_string();
    }
    let mut out: String = chars[..width.saturating_sub(1)].iter().collect();
    out.push('…');
    out
}

/// Strip ANSI escapes (used when measuring display width).
pub fn visible_width(s: &str) -> usize {
    let mut w = 0;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c == 'm' {
                esc = false;
            }
            continue;
        }
        if c == '\x1b' {
            esc = true;
            continue;
        }
        w += 1;
    }
    w
}
