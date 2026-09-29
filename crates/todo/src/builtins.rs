//! Built-in `todo.sh`-compatible commands.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use todo_core::date;
use todo_core::error::{Error, Result};
use todo_core::format::{self, ListOptions};
use todo_core::task::Task;
use todo_core::{Store, plugin};

use crate::App;

/// One-line help shown by `todo help`/`shorthelp`.
pub const BUILTIN_HELP: &[(&str, &str)] = &[
    ("add|a \"THING I NEED TO DO +project @context\"", ""),
    ("addm \"MULTI-LINE TODO\"", ""),
    ("addto DEST \"TEXT TO ADD\"", ""),
    ("append|app NR \"TEXT TO APPEND\"", ""),
    ("archive", ""),
    ("command [ACTIONS]", ""),
    ("deduplicate", ""),
    ("del|rm NR [TERM]", ""),
    ("depri|dp NR [NR ...]", ""),
    ("done|do NR [NR ...]", ""),
    ("help [ACTION...]", ""),
    ("list|ls [TERM...]", ""),
    ("listall|lsa [TERM...]", ""),
    ("listaddons", ""),
    ("listcon|lsc [TERM...]", ""),
    ("listfile|lf [SRC [TERM...]]", ""),
    ("listpri|lsp [PRIORITIES] [TERM...]", ""),
    ("listproj|lsprj [TERM...]", ""),
    ("move|mv NR DEST [SRC]", ""),
    ("prepend|prep NR \"TEXT TO PREPEND\"", ""),
    ("pri|p NR PRIORITY [NR PRIORITY ...]", ""),
    ("replace NR \"UPDATED TODO\"", ""),
    ("report", ""),
    ("plugins [NAME]", ""),
    ("shorthelp", ""),
];

pub fn is_builtin(action: &str) -> bool {
    matches!(
        action,
        "add"
            | "a"
            | "addm"
            | "addto"
            | "append"
            | "app"
            | "archive"
            | "command"
            | "deduplicate"
            | "del"
            | "rm"
            | "depri"
            | "dp"
            | "do"
            | "done"
            | "help"
            | "shorthelp"
            | "list"
            | "ls"
            | "listall"
            | "lsa"
            | "listaddons"
            | "listcon"
            | "lsc"
            | "listfile"
            | "lf"
            | "listpri"
            | "lsp"
            | "listproj"
            | "lsprj"
            | "move"
            | "mv"
            | "prepend"
            | "prep"
            | "pri"
            | "p"
            | "replace"
            | "report"
            | "plugins"
    )
}

pub fn dispatch(mut app: App, action: &str, args: &[String]) -> Result<i32> {
    match action {
        "add" | "a" => add(&mut app, args),
        "addm" => addm(&mut app, args),
        "addto" => addto(&mut app, args),
        "append" | "app" => edit_text(&mut app, args, Edit::Append),
        "prepend" | "prep" => edit_text(&mut app, args, Edit::Prepend),
        "replace" => edit_text(&mut app, args, Edit::Replace),
        "do" | "done" => done(&mut app, args),
        "del" | "rm" => delete(&mut app, args),
        "pri" | "p" => set_priority(&mut app, args),
        "depri" | "dp" => depri(&mut app, args),
        "move" | "mv" => move_task(&mut app, args),
        "list" | "ls" => list(&mut app, args, false),
        "listall" | "lsa" => list(&mut app, args, true),
        "listpri" | "lsp" => list_priority(&mut app, args),
        "listproj" | "lsprj" => list_sigil(&mut app, args, '+'),
        "listcon" | "lsc" => list_sigil(&mut app, args, '@'),
        "listfile" | "lf" => list_file(&mut app, args),
        "archive" => archive(&mut app),
        "deduplicate" => deduplicate(&mut app),
        "report" => report(&mut app),
        "listaddons" => list_addons(&app),
        "plugins" => plugins(&app, args),
        "help" | "shorthelp" => {
            crate::print_usage(&app.config);
            Ok(0)
        }
        "command" => command(&mut app, args),
        _ => {
            eprintln!("TODO: no such action: {action}");
            Ok(1)
        }
    }
}

fn save(app: &mut App) -> Result<()> {
    app.store.save()
}

fn today() -> String {
    date::today()
}

// --- add ------------------------------------------------------------------

fn add(app: &mut App, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return Err(Error::usage("usage: todo add \"TODO ITEM\""));
    }
    let text = args.join(" ");
    let line_no = app.store.next_line_no();
    let mut task = Task::new(line_no, text);
    if app.config.date_on_add {
        task.creation_date = Some(today());
    }
    if let Some(p) = app.config.priority_on_add {
        task.priority = Some(p);
    }
    let line = task.render();
    app.store.insert_line(line_no, line);
    save(app)?;
    if app.opts.verbose > 0 {
        println!("TODO: {line_no} added.");
    }
    Ok(0)
}

fn addm(app: &mut App, args: &[String]) -> Result<i32> {
    let mut text = String::new();
    if !args.is_empty() {
        text = args.join(" ");
    } else {
        std::io::stdin().read_to_string(&mut text)?;
    }
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let line_no = app.store.next_line_no();
        let mut task = Task::new(line_no, line.trim_end().to_string());
        if app.config.date_on_add {
            task.creation_date = Some(today());
        }
        if let Some(p) = app.config.priority_on_add {
            task.priority = Some(p);
        }
        let rendered = task.render();
        app.store.insert_line(line_no, rendered);
        if app.opts.verbose > 0 {
            println!("TODO: {line_no} added.");
        }
    }
    save(app)?;
    Ok(0)
}

fn addto(_app: &mut App, args: &[String]) -> Result<i32> {
    if args.len() < 2 {
        return Err(Error::usage("usage: todo addto DEST \"TODO ITEM\""));
    }
    let dest = PathBuf::from(&args[0]);
    let text = args[1..].join(" ");
    let line_no = Store::load(&dest)?.next_line_no();
    append_lines(&dest, &[text])?;
    println!("TODO: {line_no} added to {}", dest.display());
    Ok(0)
}

// --- edit -----------------------------------------------------------------

enum Edit {
    Append,
    Prepend,
    Replace,
}

fn edit_text(app: &mut App, args: &[String], kind: Edit) -> Result<i32> {
    if args.len() < 2 {
        return Err(Error::usage(
            "usage: todo <append|prepend|replace> NR \"TEXT\"",
        ));
    }
    let selector = &args[0];
    let text = args[1..].join(" ");
    let line_no = resolve(app, selector)?;
    let task = app
        .store
        .task_by_line_mut(line_no)
        .ok_or_else(|| Error::usage(format!("no task on line {line_no}")))?;
    match kind {
        Edit::Append => task.append(text.trim()),
        Edit::Prepend => task.prepend(text.trim()),
        Edit::Replace => task.set_description(text),
    }
    save(app)?;
    if app.opts.verbose > 0 {
        println!("TODO: {line_no} updated.");
    }
    Ok(0)
}

// --- done / archive -------------------------------------------------------

fn done(app: &mut App, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return Err(Error::usage("usage: todo do NR [NR ...]"));
    }
    let mut lines: Vec<usize> = args
        .iter()
        .map(|a| resolve(app, a))
        .collect::<Result<Vec<_>>>()?;
    lines.sort_unstable();
    lines.dedup();

    let date = today();
    let mut archived: Vec<Task> = Vec::new();
    for &line_no in lines.iter().rev() {
        if let Some(task) = app.store.task_by_line_mut(line_no) {
            task.mark_done(date.clone());
            if app.config.auto_archive {
                archived.push(task.clone());
                app.store.remove_line(line_no);
            }
        }
    }
    if !archived.is_empty() {
        archived.reverse();
        let lines: Vec<String> = archived.iter().map(Task::render).collect();
        append_lines(&app.config.done_file, &lines)?;
    }
    save(app)?;

    if app.opts.verbose > 0 {
        let joined: Vec<String> = lines.iter().map(|n| n.to_string()).collect();
        println!("TODO: {} marked done.", joined.join(", "));
    }
    Ok(0)
}

fn archive(app: &mut App) -> Result<i32> {
    let done: Vec<Task> = app.store.tasks().filter(|t| t.is_done()).cloned().collect();
    if done.is_empty() {
        if app.opts.verbose > 0 {
            println!("TODO: nothing to archive.");
        }
        return Ok(0);
    }
    let lines: Vec<String> = done.iter().map(Task::render).collect();
    append_lines(&app.config.done_file, &lines)?;
    app.store.lines.retain(|l| match l {
        todo_core::Line::Task(t) => !t.is_done(),
        todo_core::Line::Blank(_) => true,
    });
    app.store.reindex();
    save(app)?;
    if app.opts.verbose > 0 {
        println!("TODO: {} archived.", done.len());
    }
    Ok(0)
}

// --- delete / priority ----------------------------------------------------

fn delete(app: &mut App, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return Err(Error::usage("usage: todo del NR [TERM]"));
    }
    let line_no = resolve(app, &args[0])?;
    if let Some(term) = args.get(1) {
        let matches = app
            .store
            .task_by_line(line_no)
            .map(|t| t.render().contains(term.as_str()))
            .unwrap_or(false);
        if !matches {
            return Err(Error::usage(format!(
                "line {line_no} does not contain '{term}'"
            )));
        }
    }
    app.store.remove_line(line_no);
    save(app)?;
    if app.opts.verbose > 0 {
        println!("TODO: {line_no} deleted.");
    }
    Ok(0)
}

fn set_priority(app: &mut App, args: &[String]) -> Result<i32> {
    if args.len() < 2 || !args.len().is_multiple_of(2) {
        return Err(Error::usage(
            "usage: todo pri NR PRIORITY [NR PRIORITY ...]",
        ));
    }
    for pair in args.chunks(2) {
        let line_no = resolve(app, &pair[0])?;
        let p = pair[1]
            .chars()
            .next()
            .filter(|c| c.is_ascii_alphabetic())
            .ok_or_else(|| Error::usage("priority must be a letter"))?
            .to_ascii_uppercase();
        if let Some(task) = app.store.task_by_line_mut(line_no) {
            task.set_priority(Some(p));
        }
    }
    save(app)?;
    if app.opts.verbose > 0 {
        println!("TODO: priority updated.");
    }
    Ok(0)
}

fn depri(app: &mut App, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return Err(Error::usage("usage: todo depri NR [NR ...]"));
    }
    for selector in args {
        let line_no = resolve(app, selector)?;
        if let Some(task) = app.store.task_by_line_mut(line_no) {
            task.set_priority(None);
        }
    }
    save(app)?;
    Ok(0)
}

fn move_task(app: &mut App, args: &[String]) -> Result<i32> {
    if args.len() < 2 {
        return Err(Error::usage("usage: todo move NR DEST"));
    }
    let line_no = resolve(app, &args[0])?;
    let dest = PathBuf::from(&args[1]);
    let rendered = app
        .store
        .task_by_line(line_no)
        .map(Task::render)
        .ok_or_else(|| Error::usage(format!("no task on line {line_no}")))?;
    append_lines(&dest, &[rendered])?;
    app.store.remove_line(line_no);
    save(app)?;
    if app.opts.verbose > 0 {
        println!("TODO: {line_no} moved to {}", dest.display());
    }
    Ok(0)
}

// --- listing --------------------------------------------------------------

fn list_options(app: &App, all: bool, filters: Vec<String>) -> ListOptions {
    let mut opts = ListOptions::from_config(&app.config);
    opts.all = all;
    opts.filters = filters;
    opts.hide_context = app.opts.hide_context % 2 == 1 || app.config.hide_context % 2 == 1;
    opts.hide_project = app.opts.hide_project % 2 == 1 || app.config.hide_project % 2 == 1;
    opts
}

fn list(app: &mut App, args: &[String], all: bool) -> Result<i32> {
    let filters: Vec<String> = if app.opts.disable_filter {
        Vec::new()
    } else {
        args.to_vec()
    };
    let opts = list_options(app, all, filters);
    let palette = app.colors;
    print!("{}", format::render_list(&app.store, &opts, palette));
    Ok(0)
}

fn list_priority(app: &mut App, args: &[String]) -> Result<i32> {
    let mut priorities: Vec<char> = Vec::new();
    let mut filters: Vec<String> = Vec::new();
    for arg in args {
        if arg.len() == 1 && arg.chars().all(|c| c.is_ascii_alphabetic()) {
            priorities.push(arg.chars().next().unwrap().to_ascii_uppercase());
        } else {
            filters.push(arg.clone());
        }
    }
    let opts = ListOptions::from_config(&app.config);
    let mut selected: Vec<&Task> = app
        .store
        .open_tasks()
        .filter(|t| {
            priorities.is_empty() || t.priority.map(|p| priorities.contains(&p)).unwrap_or(false)
        })
        .filter(|t| filters.iter().all(|f| t.render().contains(f.as_str())))
        .collect();
    selected.sort_by_key(|t| t.line_no);
    let width = selected
        .iter()
        .map(|t| t.line_no.to_string().len())
        .max()
        .unwrap_or(1);
    for task in selected {
        println!(
            "{:>width$} {}",
            task.line_no,
            format::format_task(task, app.colors, opts.hide_context, opts.hide_project),
            width = width
        );
    }
    Ok(0)
}

fn list_sigil(app: &mut App, args: &[String], sigil: char) -> Result<i32> {
    let filters: Vec<String> = args.to_vec();
    let mut values: BTreeSet<&str> = BTreeSet::new();
    for task in app.store.open_tasks() {
        let iter: Box<dyn Iterator<Item = &str>> = if sigil == '+' {
            Box::new(task.projects())
        } else {
            Box::new(task.contexts())
        };
        for v in iter {
            if filters.is_empty() || filters.iter().any(|f| v.contains(f.as_str())) {
                values.insert(v);
            }
        }
    }
    for v in values {
        println!("{v}");
    }
    Ok(0)
}

fn list_file(app: &mut App, args: &[String]) -> Result<i32> {
    // `listfile [SRC [TERM...]]`; without SRC it behaves like `list`.
    if args.is_empty() {
        return list(app, &[], false);
    }
    let src = PathBuf::from(&args[0]);
    if !src.exists() {
        return list(app, args, false);
    }
    let store = Store::load(&src)?;
    let opts = list_options(app, false, args[1..].to_vec());
    let palette = app.colors;
    print!("{}", format::render_list(&store, &opts, palette));
    Ok(0)
}

fn deduplicate(app: &mut App) -> Result<i32> {
    let mut seen = std::collections::HashSet::new();
    app.store.lines.retain(|l| match l {
        todo_core::Line::Task(t) => seen.insert(t.render()),
        todo_core::Line::Blank(_) => true,
    });
    app.store.reindex();
    save(app)?;
    if app.opts.verbose > 0 {
        println!("TODO: duplicates removed.");
    }
    Ok(0)
}

fn report(app: &mut App) -> Result<i32> {
    let total = app.store.tasks().count();
    let done = app.store.tasks().filter(|t| t.is_done()).count();
    let open = total - done;
    println!("TODO: {total} total, {open} open, {done} done");
    let mut projects: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    for task in app.store.tasks() {
        for p in task.projects() {
            let entry = projects.entry(p.to_string()).or_default();
            if task.is_done() {
                entry.1 += 1;
            } else {
                entry.0 += 1;
            }
        }
    }
    for (project, (o, d)) in projects {
        println!("  {project}: {o} open, {d} done");
    }
    Ok(0)
}

fn list_addons(app: &App) -> Result<i32> {
    for name in plugin::list_names(&app.config) {
        println!("{name}");
    }
    Ok(0)
}

/// `plugins` lists discovered actions; `plugins NAME` shows one's usage.
fn plugins(app: &App, args: &[String]) -> Result<i32> {
    match args.first() {
        None => {
            for action in plugin::discover(&app.config) {
                let kind = if action.is_dir { "dir" } else { "file" };
                println!("{:<14} {kind:<8} {}", action.name, action.path.display());
            }
            Ok(0)
        }
        Some(name) => match plugin::find(&app.config, name) {
            Some(action) => {
                match action.usage(&app.config) {
                    Some(usage) => print!("{usage}"),
                    None => println!("{}", action.path.display()),
                }
                Ok(0)
            }
            None => Err(Error::NotFound(format!("no plugin named '{name}'"))),
        },
    }
}

fn command(app: &mut App, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return Err(Error::usage("usage: todo command ACTION [ARGS...]"));
    }
    let action = args[0].to_ascii_lowercase();
    let rest = &args[1..];
    if is_builtin(&action) {
        return dispatch(
            App {
                config: app.config.clone(),
                store: std::mem::take(&mut app.store),
                opts: app.opts.clone(),
                colors: app.colors,
            },
            &action,
            rest,
        );
    }
    if let Some(plugin) = plugin::find(&app.config, &action) {
        return plugin::run(&plugin, &app.config, rest);
    }
    Err(Error::usage(format!("no such action: {action}")))
}

// --- helpers --------------------------------------------------------------

/// Resolve a selector (line number or `id:`) to a concrete line number.
fn resolve(app: &App, selector: &str) -> Result<usize> {
    if let Some(task) = app.store.find(selector) {
        return Ok(task.line_no);
    }
    Err(Error::NotFound(format!("no task matches '{selector}'")))
}

fn append_lines(path: &Path, lines: &[String]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    Ok(())
}
