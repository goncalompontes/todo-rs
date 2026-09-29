use todo_core::format;
use todo_core::task::Task;
use todo_plugin::Context;

const PROJECT_USAGE: &str =
    "  projectview [TERM...]\n    Show tasks grouped by project, in priority order.";
const CONTEXT_USAGE: &str =
    "  contextview [TERM...]\n    Show tasks grouped by context, in priority order.";

pub fn project() {
    todo_plugin::main("projectview", PROJECT_USAGE, |ctx| view(ctx, '+'));
}

pub fn context() {
    todo_plugin::main("contextview", CONTEXT_USAGE, |ctx| view(ctx, '@'));
}

fn sigils(task: &Task, sigil: char) -> Vec<String> {
    let iter: Box<dyn Iterator<Item = &str>> = match sigil {
        '+' => Box::new(task.projects()),
        _ => Box::new(task.contexts()),
    };
    iter.map(|s| s[1..].to_string()).collect()
}

fn view(ctx: &mut Context, sigil: char) -> todo_core::Result<i32> {
    let mut limits: Vec<String> = Vec::new();
    let mut terms: Vec<String> = Vec::new();
    for arg in &ctx.args {
        if arg.starts_with(sigil) && arg.len() > 1 {
            limits.push(arg[1..].to_string());
        } else if !arg.is_empty() {
            terms.push(arg.to_ascii_lowercase());
        }
    }

    let matches_terms = |task: &Task| {
        terms.is_empty()
            || terms
                .iter()
                .all(|t| task.render().to_ascii_lowercase().contains(t))
    };

    let open: Vec<&Task> = ctx
        .store
        .open_tasks()
        .filter(|t| matches_terms(t))
        .collect();

    let limited = !limits.is_empty();
    let values: Vec<String> = if limited {
        std::mem::take(&mut limits)
    } else {
        let mut set = std::collections::BTreeSet::new();
        for task in &open {
            for v in sigils(task, sigil) {
                set.insert(v);
            }
        }
        set.into_iter().collect()
    };

    let (title, none_title) = if sigil == '+' {
        ("=====  Projects  =====", "--- Not in projects ---")
    } else {
        ("===== Contexts =====", "--- No context ---")
    };
    println!("{title}");
    if sigil == '+' {
        println!();
    }

    let hide_context = ctx.config.hide_context % 2 == 1;
    let hide_project = ctx.config.hide_project % 2 == 1;

    for value in &values {
        let token = format!("{sigil}{value}");
        let mut selected: Vec<&Task> = open
            .iter()
            .copied()
            .filter(|t| t.tokens().any(|tok| tok == token))
            .collect();
        if selected.is_empty() {
            continue;
        }
        crate::sort_by_priority(&mut selected);
        println!("--- {value} ---");
        for task in selected {
            println!(
                "{}",
                format::format_task(task, ctx.colors, hide_context, hide_project)
            );
        }
        if sigil == '+' {
            println!();
        }
    }

    if !limited {
        let none: Vec<&Task> = open
            .iter()
            .copied()
            .filter(|t| sigils(t, sigil).is_empty())
            .collect();
        if !none.is_empty() {
            println!("{none_title}");
            for task in none {
                println!(
                    "{}",
                    format::format_task(task, ctx.colors, hide_context, hide_project)
                );
            }
        }
    }
    Ok(0)
}
