//! `again` — complete a recurring task and re-add it with shifted dates.
//!
//! Native port of Niklas Thorne's `again` bash add-on. `again N [ADJUST]`
//! marks task `N` done and re-adds a copy whose `due:` and `t:` dates are
//! shifted by `ADJUST`:
//!
//! * `again N`        — set `due:`/`t:` to today (or use an `again:` tag).
//! * `again N 3`      — shift from _today_ by 3 days.
//! * `again N 3w`     — …by 3 weeks (units `d`, `w`, `m`, `y`, `b`; `b` is
//!   business days).
//! * `again N +3w`    — shift from the dates' _original values_ instead.
//!
//! The recurrence can also be stored on the task as `again:3w` (the tag name
//! is configurable via `TODO_AGAIN_TAG`); when no `ADJUST` is given the tag
//! value is used.

use std::io::Write;

use todo_core::date;
use todo_core::error::{Error, Result};
use todo_core::task::Task;
use todo_plugin::Context;

const USAGE: &str = "    again N
      Mark N as complete and recreate with any due date set as today.
    again N ADJUST
      Mark N as complete and recreate with any due date and deferral date
      set as ADJUST from _today_.
    again N +ADJUST
      Mark N as complete and recreate with any due date and deferral date
      set as ADJUST from _their original values_.
";

fn main() {
    todo_plugin::main("again", USAGE, run);
}

fn run(ctx: &mut Context) -> Result<i32> {
    let mut args = ctx.args.clone();
    // Accept both invocation styles: the native host passes only the real
    // arguments, the legacy wrapper receives its own name first.
    if args.first().map(String::as_str) == Some("again") {
        args.remove(0);
    }

    let item = args.first().cloned().unwrap_or_default();
    if item.is_empty() || !item.chars().all(|c| c.is_ascii_digit()) {
        eprintln!("error: {item}: invalid item number");
        return Ok(1);
    }
    let line_no: usize = item.parse().unwrap_or(0);

    if !ctx.config.file.is_file() {
        eprintln!("error: {}: no such file", ctx.config.file.display());
        return Ok(1);
    }

    let again_tag = env(ctx, "TODO_AGAIN_TAG").unwrap_or_else(|| "again".to_string());

    let Some(task) = ctx.store.task_by_line(line_no).cloned() else {
        eprintln!("error: {item}: no such line");
        return Ok(1);
    };

    // ADJUST from the command line, else from an `again:` tag on the task.
    let mut adjust: Option<String> = args.get(1).cloned().filter(|s| !s.is_empty());
    let mut copy = task;
    if adjust.is_none() {
        let line = copy.render();
        if line.contains(&format!(" {again_tag}:")) {
            adjust = tag_value(&line, &again_tag);
        }
    }

    let today = date::today();

    // Creation date: `replace_creation_date`/`remove_creation_date`.
    if ctx.config.date_on_add {
        copy.creation_date = None;
    } else if copy.creation_date.is_some() {
        copy.creation_date = Some(today.clone());
    }

    // Shift due:/t: dates.
    replace_tagged_date(&mut copy, "due", adjust.as_deref(), &today)?;
    replace_tagged_date(&mut copy, "t", adjust.as_deref(), &today)?;

    let line = copy.render();

    // `command do "$ITEM"`: mark done (and archive, per configuration).
    let mut archived: Vec<Task> = Vec::new();
    if let Some(t) = ctx.store.task_by_line_mut(line_no) {
        t.mark_done(today.clone());
        if ctx.config.auto_archive {
            archived.push(t.clone());
            ctx.store.remove_line(line_no);
        }
    }
    if !archived.is_empty() {
        let rendered: Vec<String> = archived.iter().map(Task::render).collect();
        append(&ctx.config.done_file, &rendered)?;
    }

    // `command add "$LINE"`, unless the user opted out and the task is untagged.
    let restrict = env(ctx, "TODO_NO_AGAIN_IF_NOT_TAGGED").is_some();
    let tagged = line.contains(&format!(" {again_tag}:"));
    if !line.is_empty() && (!restrict || tagged) {
        let new_line = ctx.store.next_line_no();
        // Mirror `todo.sh`'s `_addto`, which is what the bash add-on delegates
        // to: prepend today's date (after an optional priority) and add a
        // default priority only when none is present.
        let mut text = line.clone();
        if ctx.config.date_on_add {
            text = insert_add_date(&text, &today);
        }
        if let Some(p) = ctx.config.priority_on_add
            && leading_priority(&text).is_none()
        {
            text = format!("({p}) {text}");
        }
        ctx.store.insert_line(new_line, text);
    }

    ctx.store.save()?;
    Ok(0)
}

// --- date shifting --------------------------------------------------------

struct Recur {
    n: i64,
    unit: char,
    from_original: bool,
}

/// Replace the last `TAG:YYYY-MM-DD` occurrence in `task`, per the bash
/// `sed` greedy match. Does nothing when the tag carries no date.
fn replace_tagged_date(
    task: &mut Task,
    tag: &str,
    adjust: Option<&str>,
    today: &str,
) -> Result<()> {
    let Some((start, existing)) = last_tag_date(&task.description, tag) else {
        return Ok(());
    };
    let new_date = match adjust {
        None => today.to_string(),
        Some("") => today.to_string(),
        Some(a) => {
            let rec = parse_adjust(a)?;
            let base = if rec.from_original {
                existing.as_str()
            } else {
                today
            };
            adjust_date(base, &rec)?
        }
    };
    let value_at = start + tag.len() + 1;
    let end = value_at + 10;
    task.description = format!(
        "{}{}{}",
        &task.description[..value_at],
        new_date,
        &task.description[end..]
    );
    Ok(())
}

/// Last `tag:` immediately followed by a `YYYY-MM-DD`, returning the byte
/// offset just before `tag:` and the matched date.
fn last_tag_date(desc: &str, tag: &str) -> Option<(usize, String)> {
    let needle = format!("{tag}:");
    let mut best = None;
    let mut from = 0;
    while let Some(rel) = desc[from..].find(&needle) {
        let pos = from + rel;
        let after = &desc[pos + needle.len()..];
        if after.len() >= 10 && is_date10(&after[..10]) {
            best = Some((pos, after[..10].to_string()));
        }
        from = pos + needle.len();
    }
    best
}

/// Value after the last ` TAG:` (non-space run), as the bash `sed` does.
fn tag_value(line: &str, tag: &str) -> Option<String> {
    let needle = format!(" {tag}:");
    let pos = line.rfind(&needle)? + needle.len();
    let value: String = line[pos..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    Some(value)
}

fn is_date10(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Returns `Some(4)` when `text` starts with an uppercase priority `(X) `.
fn leading_priority(text: &str) -> Option<usize> {
    let b = text.as_bytes();
    (b.len() >= 4
        && b[0] == b'('
        && b[2] == b')'
        && b[3] == b' '
        && (b[1] as char).is_ascii_uppercase())
    .then_some(4)
}

/// `todo.sh`'s `s/^\(([A-Z]) \)\{0,1\}/\1$now /`: put today's date after an
/// optional leading priority.
fn insert_add_date(text: &str, today: &str) -> String {
    if leading_priority(text).is_some() {
        format!("{} {today} {}", &text[..3], &text[4..])
    } else {
        format!("{today} {text}")
    }
}

fn parse_adjust(adjust: &str) -> Result<Recur> {
    // `[[ "$ADJUST" =~ ^\+[0-9]+ ]]`
    let from_original = {
        let mut chars = adjust.chars();
        chars.next() == Some('+') && matches!(chars.next(), Some(c) if c.is_ascii_digit())
    };
    // `expr "$ADJUST" : '+*\([1-9][0-9]*\)'`
    let stripped = adjust.trim_start_matches('+');
    let mut digits = String::new();
    for c in stripped.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            break;
        }
    }
    let valid = stripped
        .chars()
        .next()
        .map(|c| c.is_ascii_digit() && c != '0')
        .unwrap_or(false);
    if !valid || digits.is_empty() {
        return Err(Error::usage(format!("invalid adjust '{adjust}'")));
    }
    let n: i64 = digits
        .parse()
        .map_err(|_| Error::usage(format!("invalid adjust '{adjust}'")))?;
    // `expr "$ADJUST" : '.*\([dwmyb]\)'`, defaulting to days.
    let unit = adjust
        .chars()
        .last()
        .filter(|c| matches!(c, 'd' | 'w' | 'm' | 'y' | 'b'))
        .unwrap_or('d');
    Ok(Recur {
        n,
        unit,
        from_original,
    })
}

fn adjust_date(base: &str, rec: &Recur) -> Result<String> {
    let days =
        date::to_unix_days(base).ok_or_else(|| Error::usage(format!("invalid date '{base}'")))?;
    match rec.unit {
        'd' => Ok(date::from_unix_days(days + rec.n)),
        'w' => Ok(date::from_unix_days(days + rec.n * 7)),
        'b' => Ok(date::from_unix_days(days + business_days(days, rec.n))),
        'm' => Ok(add_months(base, rec.n as i32)),
        'y' => Ok(add_months(base, (rec.n * 12) as i32)),
        _ => Ok(date::from_unix_days(days + rec.n)),
    }
}

/// Extra calendar days so that `n` business days later skips weekends.
/// Mirrors the `b` branch of `adjust_date` in `againHelpers.sh`.
fn business_days(unix_days: i64, n: i64) -> i64 {
    let start = (unix_days + 3).rem_euclid(7) + 1; // 1=Mon … 7=Sun (GNU `%u`)
    let extra = if start > 5 { start % 5 } else { 0 };
    let start = if start > 5 { 5 } else { start };
    let weeks = (start + n - 1) / 5;
    2 * weeks + n - extra
}

/// Add `months` to `base`, replicating the GNU "end of month" workaround.
fn add_months(base: &str, months: i32) -> String {
    let (year, month, day) = split_ymd(base);
    if day <= 28 {
        let (ny, nm) = add_ym(year, month, months);
        return format!("{ny:04}-{nm:02}-{day:02}");
    }
    let (ny, nm) = add_ym(year, month, months);
    let last = date::days_in_month(ny, nm);
    let day = day.min(last);
    format!("{ny:04}-{nm:02}-{day:02}")
}

fn split_ymd(s: &str) -> (i32, u32, u32) {
    let mut parts = s.split('-');
    let y = parts.next().and_then(|p| p.parse().ok()).unwrap_or(1970);
    let m = parts.next().and_then(|p| p.parse().ok()).unwrap_or(1);
    let d = parts.next().and_then(|p| p.parse().ok()).unwrap_or(1);
    (y, m, d)
}

fn add_ym(year: i32, month: u32, months: i32) -> (i32, u32) {
    let total = year * 12 + (month as i32 - 1) + months;
    let ny = total.div_euclid(12);
    let nm = total.rem_euclid(12) as u32 + 1;
    (ny, nm)
}

fn append(path: &std::path::Path, lines: &[String]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    Ok(())
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

    fn rec(s: &str) -> Recur {
        parse_adjust(s).unwrap()
    }

    #[test]
    fn days_weeks_and_business_days() {
        assert_eq!(adjust_date("2026-09-29", &rec("3")).unwrap(), "2026-10-02");
        assert_eq!(adjust_date("2026-09-29", &rec("2w")).unwrap(), "2026-10-13");
        // 1970-01-02 is a Friday; +1 business day lands on Monday.
        assert_eq!(adjust_date("1970-01-02", &rec("1b")).unwrap(), "1970-01-05");
        // 1970-01-03 is a Saturday.
        assert_eq!(adjust_date("1970-01-03", &rec("2b")).unwrap(), "1970-01-06");
    }

    #[test]
    fn months_and_years_clamp_to_end_of_month() {
        assert_eq!(add_months("2015-01-15", 1), "2015-02-15");
        assert_eq!(add_months("2015-01-31", 1), "2015-02-28");
        assert_eq!(add_months("2015-03-31", 1), "2015-04-30");
        assert_eq!(add_months("2016-02-29", 24), "2018-02-28");
    }

    #[test]
    fn adjust_flags_from_original_and_units() {
        let r = rec("+3w");
        assert!(r.from_original);
        assert_eq!(r.n, 3);
        assert_eq!(r.unit, 'w');
        assert!(!rec("3w").from_original);
        assert_eq!(rec("5").unit, 'd');
    }

    #[test]
    fn last_date_is_shifted() {
        let mut t = Task::parse(1, "pay rent due:2026-09-29 due:2026-09-30 again:1w");
        replace_tagged_date(&mut t, "due", Some("1w"), "2026-09-29").unwrap();
        assert_eq!(
            t.description,
            "pay rent due:2026-09-29 due:2026-10-06 again:1w"
        );
    }
}
