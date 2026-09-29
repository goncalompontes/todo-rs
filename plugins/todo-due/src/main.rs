//! `due` — list tasks by due date.
//!
//! Native port of Rebecca Morgan's `due.py` add-on. Tasks carrying a
//! `due:YYYY-MM-DD` token are grouped into overdue / due today / due tomorrow /
//! due within the next `N` days, each section sorted by date then task text.
//!
//! Usage: `due [N]` — `N` defaults to `1` (i.e. tomorrow is included), matching
//! the Python original.

use todo_core::date;
use todo_core::style;
use todo_plugin::Context;

const USAGE: &str = "  Visualize tasks by due date:
    due
      default behaviour generates a list tasks due today or overdue
      Optional argument (integer n) also shows tasks due in next n days.
";

fn main() {
    todo_plugin::main("due", USAGE, run);
}

fn run(ctx: &mut Context) -> todo_core::Result<i32> {
    let mut args = ctx.args.clone();
    // The legacy add-on is invoked with its own name as the first argument
    // (`due due 3`); the native host passes only the real arguments. Accept
    // both spellings.
    if args.first().map(String::as_str) == Some("due") {
        args.remove(0);
    }

    let future_days: i64 = match args.first() {
        None => 1,
        Some(s) => {
            if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                s.parse().unwrap_or(0)
            } else {
                eprintln!("TODO: Error: future_days argument '{s}' is not an integer");
                return Ok(1);
            }
        }
    };

    let key = env(ctx, "TODO_TXT_DUE_KEY").unwrap_or_else(|| "due".to_string());
    let today = date::to_unix_days(&date::today()).unwrap_or(0);

    // (days-since-epoch, rendered task, 1-based line number)
    let mut dated: Vec<(i64, String, usize)> = Vec::new();
    for task in ctx.store.tasks() {
        let Some(raw) = task.tag(&key) else { continue };
        if !is_date10(raw) {
            continue;
        }
        let Some(days) = date::to_unix_days(raw) else {
            continue;
        };
        dated.push((days, task.render(), task.line_no));
    }
    // The original sorts by (date, raw line).
    dated.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut overdue = Vec::new();
    let mut due_today = Vec::new();
    let mut due_tmr = Vec::new();
    let mut due_future = Vec::new();
    for entry in &dated {
        if entry.0 < today {
            overdue.push(entry);
        } else if entry.0 == today {
            due_today.push(entry);
        } else if entry.0 == today + 1 {
            due_tmr.push(entry);
        } else if entry.0 < today + future_days + 1 {
            due_future.push(entry);
        }
    }

    // `zero_pad = int(math.log10(len(content))) + 1`, i.e. the number of digits
    // of the physical line count (1 for an empty file).
    let zero_pad = {
        let n = ctx.store.lines.len();
        if n <= 1 { 1 } else { n.ilog10() as usize + 1 }
    };

    let mut first = true;
    print_section(&overdue, "Overdue tasks:", &mut first, zero_pad, ctx);
    print_section(&due_today, "Tasks due today:", &mut first, zero_pad, ctx);
    if future_days >= 1 {
        print_section(&due_tmr, "Tasks due tomorrow:", &mut first, zero_pad, ctx);
    }
    print_section(
        &due_future,
        &format!("Tasks due in the next {future_days} days:"),
        &mut first,
        zero_pad,
        ctx,
    );

    Ok(0)
}

fn print_section(
    entries: &[&(i64, String, usize)],
    title: &str,
    first: &mut bool,
    pad: usize,
    ctx: &Context,
) {
    if entries.is_empty() {
        return;
    }
    if !*first {
        println!();
    }
    *first = false;
    println!("===================================");
    println!("{title}");
    println!("===================================");
    for (_, render, line_no) in entries {
        let line = format!("{:0>width$} {}", line_no, render, width = pad);
        if ctx.colors.enabled()
            && let Some(p) = priority_in(&line)
        {
            println!("{}", ctx.colors.paint(style::priority(p), &line));
            continue;
        }
        println!("{line}");
    }
}

/// An exact `YYYY-MM-DD` match, like `\d{4}-\d{2}-\d{2}` in the original.
fn is_date10(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Find the first ` (X) ` priority marker, mirroring the Python regex
/// `\s\(([A-Z])\)\s` run over the numbered line.
fn priority_in(line: &str) -> Option<char> {
    line.as_bytes().windows(5).find_map(|w| {
        if w[0] == b' '
            && w[1] == b'('
            && w[3] == b')'
            && w[4] == b' '
            && (w[2] as char).is_ascii_uppercase()
        {
            Some(w[2] as char)
        } else {
            None
        }
    })
}

fn env(ctx: &Context, key: &str) -> Option<String> {
    ctx.config
        .env
        .get(key)
        .filter(|v| !v.is_empty())
        .cloned()
        .or_else(|| std::env::var(key).ok().filter(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_exact_dates_only() {
        assert!(is_date10("2026-09-29"));
        assert!(!is_date10("2026-9-29"));
        assert!(!is_date10("2026-09-29 "));
        assert!(!is_date10("next-week"));
    }

    #[test]
    fn finds_priority_on_a_numbered_line() {
        assert_eq!(priority_in("01 (A) do it"), Some('A'));
        assert_eq!(priority_in("12 something (B) else"), Some('B'));
        assert_eq!(priority_in("(A) no leading space"), None);
        assert_eq!(priority_in("01 plain task"), None);
    }
}
