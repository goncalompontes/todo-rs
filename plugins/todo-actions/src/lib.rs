//! Native Rust ports of the shell/Python add-ons, packaged as plugins.

pub mod check;
pub mod dep;
pub mod finish;
pub mod here;
pub mod hide;
pub mod views;
pub mod vocab;

use todo_core::format;
use todo_core::paths;
use todo_core::task::Task;
use todo_plugin::Context;

/// Resolve the OSC-8 link template (`TODO_LINK_URL`, `link-url`, or file://).
pub fn link_template() -> String {
    if let Ok(v) = std::env::var("TODO_LINK_URL")
        && !v.is_empty()
    {
        return v;
    }
    let path = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .map(|d| d.join("todo/link-url"));
    if let Some(p) = path
        && let Ok(v) = std::fs::read_to_string(&p)
    {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    "file://{abs}".to_string()
}

pub fn links_enabled() -> bool {
    match std::env::var("DEP_LINKS").ok().as_deref() {
        Some("1") | Some("yes") | Some("always") => true,
        Some("0") | Some("no") | Some("never") => false,
        _ => std::io::IsTerminal::is_terminal(&std::io::stdout()),
    }
}

/// Render a task line with path tokens turned into OSC-8 hyperlinks.
pub fn linkify_line(ctx: &Context, task: &Task) -> String {
    let template = link_template();
    let enabled = links_enabled();
    // The full line (priority, dates and description), like the legacy scripts.
    let rendered = task.render();
    let mut out = Vec::new();
    for tok in rendered.split_whitespace() {
        if let Some(r) = paths::parse(tok) {
            let abs = paths::absolute(&ctx.config.dir, &r);
            let url = paths::build_url(&template, &abs, r.line, r.col);
            out.push(paths::osc8(&url, tok, enabled));
        } else {
            out.push(tok.to_string());
        }
    }
    out.join(" ")
}

pub fn priority_rank(p: Option<char>) -> u8 {
    match p {
        Some('A') => 0,
        Some('B') => 1,
        Some('C') => 2,
        Some(_) => 3,
        None => 4,
    }
}

pub fn sort_by_priority(tasks: &mut [&Task]) {
    tasks.sort_by(|a, b| {
        priority_rank(a.priority)
            .cmp(&priority_rank(b.priority))
            .then(a.line_no.cmp(&b.line_no))
    });
}

/// Print a numbered task list, right-aligning numbers.
pub fn print_numbered(ctx: &Context, tasks: &[&Task], linkified: bool) {
    let width = tasks
        .iter()
        .map(|t| t.line_no.to_string().len())
        .max()
        .unwrap_or(1);
    let hide_context = ctx.config.hide_context % 2 == 1;
    let hide_project = ctx.config.hide_project % 2 == 1;
    for task in tasks {
        let _ = linkified;
        let text = format::format_task(task, ctx.colors, hide_context, hide_project);
        println!("{:>width$} {}", task.line_no, text, width = width);
    }
}
