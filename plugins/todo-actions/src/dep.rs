//! Dependency commands: `dep`/`blocked`/`ready`/`next`/`tree` plus mutation.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use todo_core::deps::{Child, DepGraph, State};
use todo_core::format;
use todo_core::paths;
use todo_core::plugin;
use todo_core::style;
use todo_core::task::Task;
use todo_plugin::Context;

const USAGE: &str = "  dep [blocked|ready|next|tree|add|rm|id|sync|validate|graph] [opts]";

pub fn run() {
    let invoked = todo_plugin::invoked_name();
    let name = invoked.clone();
    todo_plugin::main(&invoked, USAGE, move |ctx| dispatch(ctx, &name));
}

fn dispatch(ctx: &mut Context, invoked: &str) -> todo_core::Result<i32> {
    let (sub, args): (String, Vec<String>) = match invoked {
        "blocked" | "ready" | "next" => (invoked.to_string(), ctx.args.clone()),
        _ => {
            let sub = ctx
                .args
                .first()
                .cloned()
                .unwrap_or_else(|| "blocked".to_string());
            (sub, ctx.args.get(1..).unwrap_or(&[]).to_vec())
        }
    };
    match sub.as_str() {
        "blocked" | "list" | "ls" => blocked(ctx),
        "ready" => ready(ctx),
        "next" => next(ctx, &args),
        "add" => add(ctx, &args),
        "rm" | "remove" | "del" => rm(ctx, &args),
        "id" => set_id(ctx, &args),
        "sync" => sync(ctx),
        "validate" => validate(ctx),
        "tree" => tree(ctx, &args),
        "graph" => graph(ctx, &args),
        "help" | "usage" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(0)
        }
        other => {
            eprintln!("TODO: unknown dep subcommand '{other}'");
            Ok(1)
        }
    }
}

fn state_glyph(ctx: &Context, state: State) -> String {
    match state {
        State::Blocked => ctx.colors.wrap(style::red(), "✗"),
        State::Ready => ctx.colors.wrap(style::green(), "●"),
        State::Done => ctx.colors.wrap(style::dim(), "✓"),
    }
}

fn blocked(ctx: &mut Context) -> todo_core::Result<i32> {
    let g = ctx.graph();
    let mut any = false;
    for &i in &g.open {
        let reasons = g.reasons(i);
        if reasons.is_empty() {
            continue;
        }
        any = true;
        let task = g.task(i);
        println!(
            "{} {} {}",
            state_glyph(ctx, State::Blocked),
            ctx.colors.wrap(style::dim(), format!("#{}", task.line_no)),
            crate::linkify_line(ctx, task)
        );
        for reason in reasons {
            println!("    {}↳{} {reason}", ctx.colors.dim(), ctx.colors.rst());
        }
    }
    if !any {
        println!("{}nothing blocked{}", ctx.colors.dim(), ctx.colors.rst());
    }
    Ok(0)
}

fn ready(ctx: &mut Context) -> todo_core::Result<i32> {
    let g = ctx.graph();
    for &i in &g.open {
        if !g.reasons(i).is_empty() {
            continue;
        }
        let task = g.task(i);
        println!(
            "{} {} {}",
            state_glyph(ctx, State::Ready),
            ctx.colors.wrap(style::dim(), format!("#{}", task.line_no)),
            crate::linkify_line(ctx, task)
        );
    }
    Ok(0)
}

fn next(ctx: &mut Context, args: &[String]) -> todo_core::Result<i32> {
    let mut limit = 5usize;
    let mut target: Option<String> = None;
    let mut here = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--here" => here = true,
            "--path" => {
                i += 1;
                target = args.get(i).cloned();
            }
            other => {
                if let Some(v) = other.strip_prefix("--path=") {
                    target = Some(v.to_string());
                } else if let Ok(n) = other.parse::<usize>() {
                    limit = n;
                }
            }
        }
        i += 1;
    }
    let base = ctx.config.dir.clone();
    if here && target.is_none() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| base.clone());
        target = Some(todo_core::paths::cwd_rel(&cwd, &base));
    }

    let g = ctx.graph();
    let rows = g.rank_next(limit, target.as_deref(), &base);
    if rows.is_empty() {
        println!("{}no ready tasks{}", ctx.colors.dim(), ctx.colors.rst());
        return Ok(0);
    }

    let scope = target
        .as_deref()
        .map(|t| format!(" · in {t}"))
        .unwrap_or_default();
    println!(
        "{}TODO: what to work on next{} {}(ready, ranked){}",
        ctx.colors.bold(),
        ctx.colors.rst(),
        ctx.colors.dim(),
        scope
    );
    for (rank, row) in rows.iter().enumerate() {
        let task = g.task(row.index);
        let ident = match &row.id {
            Some(id) => format!("#{} {}", task.line_no, id),
            None => format!("#{}", task.line_no),
        };
        println!(
            " {:>2}. {}{}{} {}",
            rank + 1,
            ctx.colors.bold(),
            ident,
            ctx.colors.rst(),
            task_desc(task)
        );
        let mut reasons: Vec<String> = Vec::new();
        if row.unblocks > 0 {
            reasons.push(format!("unblocks {}", row.unblocks));
        }
        if row.group_clears > 0 {
            reasons.push(format!("clears a group ({} waiting)", row.group_clears));
        }
        if let Some(p) = row.priority {
            reasons.push(format!("prio {p}"));
        }
        if let Some(s) = row.severity {
            reasons.push(format!("sev:{s}"));
        }
        if let Some(d) = &row.due {
            reasons.push(format!("due:{d}"));
        }
        let lk = first_path_token(task)
            .map(|tok| path_link(ctx, &tok))
            .unwrap_or_default();
        if !reasons.is_empty() || !lk.is_empty() {
            let joined = reasons.join(" · ");
            let sep = if !reasons.is_empty() && !lk.is_empty() {
                " · "
            } else {
                ""
            };
            println!(
                "     {}{joined}{sep}{lk}{}",
                ctx.colors.dim(),
                ctx.colors.rst()
            );
        }
    }
    println!(
        "{}(mark one done with: t do <#N>){}",
        ctx.colors.dim(),
        ctx.colors.rst()
    );
    Ok(0)
}

/// Dependency validation, matching the legacy `dep validate` output (two-space
/// indented issues, silent when everything is fine).
fn validate(ctx: &mut Context) -> todo_core::Result<i32> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut known: HashMap<String, bool> = HashMap::new();
    for t in ctx.store.tasks().chain(ctx.done.iter()) {
        if let Some(id) = t.id() {
            *counts.entry(id.to_string()).or_insert(0) += 1;
            known.entry(id.to_string()).or_insert(!t.is_done());
        }
    }

    let mut rc = 0;
    for (id, n) in &counts {
        if *n > 1 {
            println!("  duplicate id:{id} ({n}x)");
            rc = 1;
        }
    }

    let open: Vec<&Task> = ctx.store.open_tasks().collect();
    for t in &open {
        let own = t.id();
        for dep in t.depends() {
            if matches!(dep.chars().next(), Some('+' | '@' | '%')) {
                continue;
            }
            if !known.contains_key(dep) {
                println!("  line {}: depends on unknown id:{dep}", t.line_no);
                rc = 1;
            } else if Some(dep) == own {
                println!("  line {}: self-dependency id:{dep}", t.line_no);
                rc = 1;
            }
        }
        for parent in t.parents() {
            if !known.contains_key(parent) {
                println!("  line {}: p:{parent} refers to unknown id", t.line_no);
                rc = 1;
            } else if Some(parent) == own {
                println!("  line {}: p:{parent} is a self-parent", t.line_no);
                rc = 1;
            }
        }
    }

    if let Some(names) = dep_cycle(&open) {
        println!("  dependency cycle: {}", names.join(" -> "));
        rc = 1;
    }
    Ok(rc)
}

fn dep_cycle(open: &[&Task]) -> Option<Vec<String>> {
    let graph: HashMap<&str, Vec<&str>> = open
        .iter()
        .filter_map(|t| {
            let id = t.id()?;
            Some((
                id,
                t.depends()
                    .into_iter()
                    .filter(|d| !matches!(d.chars().next(), Some('+' | '@' | '%')))
                    .collect(),
            ))
        })
        .collect();
    let mut visiting: HashSet<&str> = HashSet::new();
    let mut done: HashSet<&str> = HashSet::new();
    for &start in graph.keys() {
        if done.contains(start) {
            continue;
        }
        let mut stack: Vec<&str> = vec![start];
        visiting.insert(start);
        while let Some(&cur) = stack.last() {
            if let Some(next) = graph.get(cur)
                && let Some(unvisited) = next.iter().copied().find(|n| !done.contains(n))
            {
                if visiting.contains(unvisited) {
                    let mut names: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
                    names.push(unvisited.to_string());
                    return Some(names);
                }
                visiting.insert(unvisited);
                stack.push(unvisited);
                continue;
            }
            stack.pop();
            visiting.remove(cur);
            done.insert(cur);
        }
    }
    None
}

// --- mutation -------------------------------------------------------------

fn resolve(ctx: &Context, selector: &str) -> todo_core::Result<usize> {
    ctx.store
        .find(selector)
        .map(|t| t.line_no)
        .ok_or_else(|| todo_core::Error::NotFound(format!("no task matches '{selector}'")))
}

fn add(ctx: &mut Context, args: &[String]) -> todo_core::Result<i32> {
    if args.len() < 2 {
        return Err(todo_core::Error::usage("usage: todo dep add SEL TARGET"));
    }
    let line = resolve(ctx, &args[0])?;
    let target = &args[1];
    let own_id = ctx
        .store
        .task_by_line(line)
        .and_then(|t| t.id().map(str::to_string));
    if let Some(task) = ctx.store.task_by_line_mut(line) {
        task.add_desc_token(&format!("depends:{target}"));
    }
    if !matches!(target.chars().next(), Some('+' | '@' | '%'))
        && let (Some(own_id), Some(target_line)) =
            (own_id, ctx.store.task_by_id(target).map(|t| t.line_no))
        && let Some(task) = ctx.store.task_by_line_mut(target_line)
    {
        task.add_desc_token(&format!("p:{own_id}"));
    }
    ctx.store.save()?;
    Ok(0)
}

fn rm(ctx: &mut Context, args: &[String]) -> todo_core::Result<i32> {
    if args.len() < 2 {
        return Err(todo_core::Error::usage("usage: todo dep rm SEL TARGET"));
    }
    let line = resolve(ctx, &args[0])?;
    let target = &args[1];
    let own_id = ctx
        .store
        .task_by_line(line)
        .and_then(|t| t.id().map(str::to_string));
    if let Some(task) = ctx.store.task_by_line_mut(line) {
        task.remove_desc_token(&format!("depends:{target}"));
        task.remove_desc_token(&format!("dep:{target}"));
    }
    if let (Some(own_id), Some(target_line)) =
        (own_id, ctx.store.task_by_id(target).map(|t| t.line_no))
        && let Some(task) = ctx.store.task_by_line_mut(target_line)
    {
        task.remove_desc_token(&format!("p:{own_id}"));
    }
    ctx.store.save()?;
    Ok(0)
}

fn set_id(ctx: &mut Context, args: &[String]) -> todo_core::Result<i32> {
    if args.len() < 2 {
        return Err(todo_core::Error::usage("usage: todo dep id SEL SLUG"));
    }
    let line = resolve(ctx, &args[0])?;
    if let Some(task) = ctx.store.task_by_line_mut(line) {
        task.set_tag("id", &args[1]);
    }
    ctx.store.save()?;
    Ok(0)
}

/// Rebuild `p:` parent mirrors from fine `depends:` links.
fn sync(ctx: &mut Context) -> todo_core::Result<i32> {
    let mut parents: HashMap<String, Vec<String>> = HashMap::new();
    for task in ctx.store.open_tasks() {
        let Some(id) = task.id() else { continue };
        for dep in task.depends() {
            if matches!(dep.chars().next(), Some('+' | '@' | '%')) {
                continue;
            }
            parents
                .entry(dep.to_string())
                .or_default()
                .push(id.to_string());
        }
    }
    for task in ctx.store.tasks_mut() {
        task.remove_tokens_with_prefix("p:");
    }
    let ids: Vec<(usize, String)> = ctx
        .store
        .tasks()
        .filter_map(|t| t.id().map(|id| (t.line_no, id.to_string())))
        .collect();
    for (line, id) in ids {
        if let Some(parent_ids) = parents.get(&id)
            && let Some(task) = ctx.store.task_by_line_mut(line)
        {
            for p in parent_ids {
                task.add_desc_token(&format!("p:{p}"));
            }
        }
    }
    ctx.store.save()?;
    Ok(0)
}

fn graph(ctx: &mut Context, args: &[String]) -> todo_core::Result<i32> {
    if let Some(action) = plugin::find(&ctx.config, "graph") {
        let mut full = vec!["graph".to_string()];
        full.extend(args.iter().cloned());
        return plugin::run(&action, &ctx.config, &full);
    }
    eprintln!("TODO: graph plugin not installed");
    Ok(1)
}

// --- tree -----------------------------------------------------------------

#[derive(Clone)]
enum Node {
    Task(usize, bool), // (index, is_ref)
    Group(String, bool),
    Missing(String),
}

struct Tree<'a> {
    graph: DepGraph<'a>,
    seen_tasks: HashSet<usize>,
    seen_groups: HashSet<String>,
    kids: HashMap<usize, Vec<Node>>,
    gkids: HashMap<String, Vec<Node>>,
}

impl<'a> Tree<'a> {
    fn new(graph: DepGraph<'a>) -> Self {
        Tree {
            graph,
            seen_tasks: HashSet::new(),
            seen_groups: HashSet::new(),
            kids: HashMap::new(),
            gkids: HashMap::new(),
        }
    }

    /// Returns true when the task was newly expanded (not a repeat).
    fn build_task(&mut self, i: usize) -> bool {
        if !self.seen_tasks.insert(i) {
            return false;
        }
        let children = self.graph.children(i);
        for child in children {
            match child {
                Child::Task(j) => {
                    let expanded = self.build_task(j);
                    self.kids
                        .entry(i)
                        .or_default()
                        .push(Node::Task(j, !expanded));
                }
                Child::Group(tag) => {
                    if self.seen_groups.insert(tag.clone()) {
                        self.build_group(&tag, i);
                        self.kids
                            .entry(i)
                            .or_default()
                            .push(Node::Group(tag, false));
                    } else {
                        self.kids.entry(i).or_default().push(Node::Group(tag, true));
                    }
                }
                Child::Missing(m) => self.kids.entry(i).or_default().push(Node::Missing(m)),
            }
        }
        true
    }

    fn build_group(&mut self, tag: &str, parent: usize) {
        let members = self.graph.group_members(tag, parent);
        for m in members {
            let expanded = self.build_task(m);
            self.gkids
                .entry(tag.to_string())
                .or_default()
                .push(Node::Task(m, !expanded));
        }
    }
}

fn tree(ctx: &mut Context, args: &[String]) -> todo_core::Result<i32> {
    let mut group_by = String::new();
    let mut only_blocked = false;
    let mut include_indep = true;
    let mut dedup_flag: Option<bool> = None;
    let mut desc_mode = env_width();
    let mut sel: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--blocked" => only_blocked = true,
            "--dep-only" | "--no-standalone" => include_indep = false,
            "--all" => include_indep = true,
            "--no-dedup" | "--repeat" => dedup_flag = Some(true),
            "--dedup" => dedup_flag = Some(false),
            "--full" => desc_mode = DescWidth::Full,
            "--group-by" => {
                i += 1;
                group_by = args.get(i).cloned().unwrap_or_default();
            }
            "--depth" => i += 1,
            "--width" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    desc_mode = parse_width(v).unwrap_or(desc_mode);
                }
            }
            other if other.starts_with("--group-by=") => {
                group_by = other["--group-by=".len()..].to_string()
            }
            other if other.starts_with("--width=") => {
                desc_mode = parse_width(&other["--width=".len()..]).unwrap_or(desc_mode);
            }
            other if other.starts_with("--") => {}
            other => sel = Some(other.to_string()),
        }
        i += 1;
    }
    let _ = DESC_WIDTH.set(desc_mode);
    // Legacy default: a selector shows its full tree; without one the shared
    // nodes are de-duplicated.
    let no_dedup = dedup_flag.unwrap_or(sel.is_some());
    let g = ctx.graph();
    let mut tree = Tree::new(g);
    let base = ctx.config.dir.clone();

    let roots: Vec<usize> = match &sel {
        Some(s) => match tree
            .graph
            .all
            .iter()
            .position(|t| t.id() == Some(s.as_str()) || t.line_no.to_string() == *s)
        {
            Some(i) => vec![i],
            None => {
                eprintln!("TODO: no task matches '{s}'");
                return Ok(1);
            }
        },
        None => {
            let mut roots = tree.graph.roots();
            if only_blocked {
                roots.retain(|&i| tree.graph.state(i) == State::Blocked);
            }
            roots
        }
    };
    let indep: Vec<usize> = if sel.is_none() && include_indep && !only_blocked {
        tree.graph.independents()
    } else {
        Vec::new()
    };

    if roots.is_empty() && indep.is_empty() {
        println!("{}no open tasks{}", ctx.colors.dim(), ctx.colors.rst());
        return Ok(0);
    }

    if group_by.is_empty() {
        for &r in &roots {
            emit_root(ctx, &mut tree, r, no_dedup, base.as_path());
            println!();
        }
        if !indep.is_empty() {
            println!(
                "\n{}independent (no dependencies){}",
                ctx.colors.bold(),
                ctx.colors.rst()
            );
            for &r in &indep {
                emit_root(ctx, &mut tree, r, no_dedup, base.as_path());
            }
            println!();
        }
    } else {
        // (root index, is_tree): trees get a blank line after them, the
        // independent `item:` entries do not (matching the legacy renderer).
        let mut sections: std::collections::BTreeMap<String, Vec<(usize, bool)>> =
            Default::default();
        for &r in &roots {
            for key in group_values(ctx, &tree, r, &group_by) {
                sections.entry(key).or_default().push((r, true));
            }
        }
        for &r in &indep {
            for key in group_values(ctx, &tree, r, &group_by) {
                sections.entry(key).or_default().push((r, false));
            }
        }
        for (key, members) in &sections {
            println!("\n{}{}{}", ctx.colors.bold(), key, ctx.colors.rst());
            for &(r, is_tree) in members {
                emit_root(ctx, &mut tree, r, no_dedup, base.as_path());
                if is_tree {
                    println!();
                }
            }
        }
        println!();
    }

    // summary
    let total = ctx.store.open_tasks().count();
    let blocked = tree
        .graph
        .open
        .iter()
        .filter(|&&i| tree.graph.state(i) == State::Blocked)
        .count();
    let ready = total - blocked;
    let hubs: HashSet<String> = tree
        .graph
        .open
        .iter()
        .flat_map(|&i| {
            tree.graph
                .task(i)
                .group_depends()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    println!(
        "{}{total} open · {blocked} blocked · {ready} ready · {} independent · {} group hub(s){}",
        ctx.colors.dim(),
        indep.len(),
        hubs.len(),
        ctx.colors.rst()
    );
    Ok(0)
}

fn group_values(ctx: &Context, tree: &Tree<'_>, i: usize, by: &str) -> Vec<String> {
    let task = tree.graph.task(i);
    let mut out = Vec::new();
    match by {
        "module" | "project" => out.extend(task.projects().map(str::to_string)),
        "kind" | "context" => out.extend(task.contexts().map(str::to_string)),
        "group" => out.extend(task.groups().map(str::to_string)),
        "priority" => out.extend(task.priority.map(|p| format!("({p})"))),
        "status" => out.push(match tree.graph.state(i) {
            State::Blocked => "blocked".to_string(),
            State::Ready => "ready".to_string(),
            State::Done => "done".to_string(),
        }),
        _ => {}
    }
    let _ = ctx;
    if out.is_empty() {
        out.push("(none)".to_string());
    }
    out
}

fn child_prefix(prefix: &str, branch: &str) -> String {
    match branch {
        "" => prefix.to_string(),
        "└─ " => format!("{prefix}   "),
        _ => format!("{prefix}│  "),
    }
}

#[derive(Debug, Clone, Copy)]
enum DescWidth {
    Auto,
    Full,
    Fixed(usize),
}

static DESC_WIDTH: OnceLock<DescWidth> = OnceLock::new();

fn parse_width(s: &str) -> Option<DescWidth> {
    match s.trim() {
        "auto" | "" => Some(DescWidth::Auto),
        "full" | "none" => Some(DescWidth::Full),
        n => n.parse().ok().map(DescWidth::Fixed),
    }
}

fn env_width() -> DescWidth {
    std::env::var("DEP_DESC_WIDTH")
        .ok()
        .and_then(|v| parse_width(&v))
        .unwrap_or(DescWidth::Auto)
}

static COLS: OnceLock<usize> = OnceLock::new();

/// Terminal columns, following the legacy `dep` chain: `/dev/tty`, then
/// `tput cols`, then `$COLUMNS`, then 100. Cached for the run.
fn term_cols() -> usize {
    *COLS.get_or_init(|| {
        if let Ok(tty) = std::fs::File::open("/dev/tty")
            && let Some((w, _)) = terminal_size::terminal_size_of(&tty)
            && w.0 > 0
        {
            return w.0 as usize;
        }
        if let Ok(out) = std::process::Command::new("tput").arg("cols").output()
            && out.status.success()
            && let Ok(n) = String::from_utf8_lossy(&out.stdout).trim().parse::<usize>()
            && n > 0
        {
            return n;
        }
        if let Ok(c) = std::env::var("COLUMNS")
            && let Ok(n) = c.trim().parse::<usize>()
            && n > 0
        {
            return n;
        }
        100
    })
}

/// Available description width, matching `dep`'s `desc_width`.
fn desc_width(prefix: usize, id: Option<usize>, n: usize, badge: usize, reserve: usize) -> usize {
    match DESC_WIDTH.get().copied().unwrap_or(DescWidth::Auto) {
        DescWidth::Full => return 0,
        DescWidth::Fixed(w) => return w,
        DescWidth::Auto => {}
    }
    let cols = term_cols() as isize;
    let mut avail = cols - (prefix as isize + 6 + badge as isize + reserve as isize);
    if let Some(id) = id {
        avail -= id as isize + 1;
    }
    avail -= n.to_string().len() as isize + 1;
    if avail < 12 { 12 } else { avail as usize }
}

/// A `+project` / `@context` / `%group` / `key:value` token.
fn is_meta_token(tok: &str) -> bool {
    if tok.starts_with(['+', '@', '%']) {
        return true;
    }
    if let Some((k, v)) = tok.split_once(':') {
        // The legacy regex requires a non-empty value (`:[^ ]+`), so a bare
        // `word:` stays part of the description.
        if !v.is_empty() {
            let mut it = k.chars();
            if let Some(c) = it.next()
                && (c.is_ascii_alphabetic() || c == '_')
                && it.all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return true;
            }
        }
    }
    false
}

/// The task text with all metadata tokens removed (legacy `task_desc`).
fn task_desc(task: &Task) -> String {
    task.tokens()
        .filter(|t| !is_meta_token(t))
        .collect::<Vec<_>>()
        .join(" ")
}

fn first_path_token(task: &Task) -> Option<String> {
    task.tokens()
        .find(|t| t.starts_with("file:") || t.starts_with("dir:") || t.starts_with("path:"))
        .map(str::to_string)
}

fn path_link(ctx: &Context, token: &str) -> String {
    match paths::parse(token) {
        Some(r) => {
            let abs = paths::absolute(&ctx.config.dir, &r);
            let url = paths::build_url(&crate::link_template(), &abs, r.line, r.col);
            paths::osc8(&url, token, crate::links_enabled())
        }
        None => token.to_string(),
    }
}

fn badge_text(state: State) -> &'static str {
    match state {
        State::Blocked => "[blocked]",
        State::Ready => "[ready]",
        State::Done => "[done]",
    }
}

fn badge_style(state: State) -> style::Style {
    match state {
        State::Blocked => style::red(),
        State::Ready => style::green(),
        State::Done => style::dim(),
    }
}

/// The label after the glyph: `id #n <truncated desc> [state] · path`, matching
/// the legacy `dep` tree output rather than the raw task line.
fn tree_label(ctx: &Context, task: &Task, prefix: &str, state: State) -> String {
    let id = task.id();
    let pvis = first_path_token(task);
    let reserve = pvis.as_ref().map(|p| p.chars().count() + 3).unwrap_or(0);
    let badge = badge_text(state);
    let width = desc_width(
        prefix.chars().count(),
        id.map(|s| s.chars().count()),
        task.line_no,
        badge.chars().count(),
        reserve,
    );
    let desc = format::truncate(&task_desc(task), width);

    let mut label = String::new();
    if let Some(id) = id {
        label.push_str(&ctx.colors.paint(style::bold(), format!("{id} ")));
    }
    label.push_str(&ctx.colors.paint(style::dim(), format!("#{}", task.line_no)));
    label.push(' ');
    label.push_str(&desc);
    label.push(' ');
    label.push_str(&ctx.colors.paint(badge_style(state), badge));
    if let Some(tok) = pvis {
        label.push(' ');
        label.push_str(&ctx.colors.dim());
        label.push('·');
        label.push_str(&ctx.colors.rst());
        label.push(' ');
        label.push_str(&path_link(ctx, &tok));
    }
    label
}

fn render_task(
    ctx: &Context,
    tree: &Tree<'_>,
    i: usize,
    prefix: &str,
    branch: &str,
    depth: usize,
    base: &std::path::Path,
) {
    let task = tree.graph.task(i);
    let state = tree.graph.state(i);
    let label = tree_label(ctx, task, prefix, state);
    println!("{prefix}{branch}{} {label}", state_glyph(ctx, state));
    if depth >= 20 {
        return;
    }
    let childp = child_prefix(prefix, branch);
    let children = tree.kids.get(&i).cloned().unwrap_or_default();
    let count = children.len();
    for (idx, node) in children.into_iter().enumerate() {
        let last = idx + 1 == count;
        let b = if last { "└─ " } else { "├─ " };
        render_node(ctx, tree, &node, &childp, b, depth + 1, base);
    }
}

fn render_node(
    ctx: &Context,
    tree: &Tree<'_>,
    node: &Node,
    prefix: &str,
    branch: &str,
    depth: usize,
    base: &std::path::Path,
) {
    match node {
        Node::Task(j, true) => render_ref_task(ctx, tree, *j, prefix, branch),
        Node::Task(j, false) => render_task(ctx, tree, *j, prefix, branch, depth, base),
        Node::Group(tag, true) => println!(
            "{prefix}{branch}{} {}{}(shown above){}",
            ctx.colors.wrap(style::yellow(), "↳"),
            ctx.colors.cyan(),
            tag,
            ctx.colors.rst()
        ),
        Node::Group(tag, false) => {
            let members = tree.gkids.get(tag).cloned().unwrap_or_default();
            let total = tree.graph.group_members(tag, 0).len();
            println!(
                "{prefix}{branch}{} {}({total} open){}",
                ctx.colors.wrap(style::magenta(), tag),
                ctx.colors.dim(),
                ctx.colors.rst()
            );
            let childp = child_prefix(prefix, branch);
            let count = members.len();
            for (idx, node) in members.into_iter().enumerate() {
                let last = idx + 1 == count;
                let b = if last { "└─ " } else { "├─ " };
                render_node(ctx, tree, &node, &childp, b, depth + 1, base);
            }
        }
        Node::Missing(m) => println!(
            "{prefix}{branch}{}{}(MISSING){}",
            ctx.colors.red(),
            m,
            ctx.colors.rst()
        ),
    }
}

/// Reference-line label: no status badge, legacy `· path` form.
fn ref_label(ctx: &Context, task: &Task) -> String {
    let mut label = String::new();
    if let Some(id) = task.id() {
        label.push_str(&ctx.colors.paint(style::bold(), format!("{id} ")));
    }
    label.push_str(&ctx.colors.paint(style::dim(), format!("#{}", task.line_no)));
    label.push(' ');
    label.push_str(&task_desc(task));
    if let Some(tok) = first_path_token(task) {
        label.push(' ');
        label.push_str(&ctx.colors.paint(style::dim(), format!("· {tok}")));
    }
    label
}

fn render_ref_task(ctx: &Context, tree: &Tree<'_>, i: usize, prefix: &str, branch: &str) {
    let task = tree.graph.task(i);
    let label = ref_label(ctx, task);
    println!(
        "{p}{b}{arrow} {label} {dim}(shown above){rst}",
        p = prefix,
        b = branch,
        arrow = ctx.colors.wrap(style::yellow(), "↳"),
        label = label,
        dim = ctx.colors.dim(),
        rst = ctx.colors.rst(),
    );
}

/// Emit one root, either de-duplicated (shared nodes as references) or, with
/// `--no-dedup`, as a full repeating tree like the legacy `dep`.
fn emit_root(ctx: &Context, tree: &mut Tree<'_>, r: usize, no_dedup: bool, base: &Path) {
    if no_dedup {
        let mut path = Vec::new();
        render_full_task(ctx, &tree.graph, r, "", "", 1, &mut path);
    } else if tree.build_task(r) {
        render_task(ctx, tree, r, "", "", 1, base);
    } else {
        render_ref_task(ctx, tree, r, "", "");
    }
}

fn render_full_task(
    ctx: &Context,
    g: &DepGraph<'_>,
    i: usize,
    prefix: &str,
    branch: &str,
    depth: usize,
    path: &mut Vec<usize>,
) {
    let task = g.task(i);
    let state = g.state(i);
    let label = tree_label(ctx, task, prefix, state);
    println!("{prefix}{branch}{} {label}", state_glyph(ctx, state));
    if depth >= 30 {
        println!("{prefix}   ...");
        return;
    }
    path.push(i);
    let cp = child_prefix(prefix, branch);
    let children = g.children(i);
    let count = children.len();
    for (idx, child) in children.into_iter().enumerate() {
        let last = idx + 1 == count;
        let b = if last { "└─ " } else { "├─ " };
        match child {
            Child::Task(j) => {
                if path.contains(&j) {
                    println!("{cp}{b}{}", ctx.colors.wrap(style::yellow(), "(cycle)"));
                } else {
                    render_full_task(ctx, g, j, &cp, b, depth + 1, path);
                }
            }
            Child::Group(tag) => render_full_group(ctx, g, &tag, i, &cp, b, depth + 1, path),
            Child::Missing(m) => {
                println!(
                    "{cp}{b}{}{}(MISSING){}",
                    ctx.colors.red(),
                    m,
                    ctx.colors.rst()
                )
            }
        }
    }
    path.pop();
}

#[allow(clippy::too_many_arguments)]
fn render_full_group(
    ctx: &Context,
    g: &DepGraph<'_>,
    tag: &str,
    parent: usize,
    prefix: &str,
    branch: &str,
    depth: usize,
    path: &mut Vec<usize>,
) {
    let members = g.group_members(tag, parent);
    println!(
        "{prefix}{branch}{} {}({} open){}",
        ctx.colors.wrap(style::magenta(), tag),
        ctx.colors.dim(),
        members.len(),
        ctx.colors.rst()
    );
    if depth >= 30 {
        println!("{prefix}   ...");
        return;
    }
    let cp = child_prefix(prefix, branch);
    let count = members.len();
    for (idx, m) in members.into_iter().enumerate() {
        let last = idx + 1 == count;
        let b = if last { "└─ " } else { "├─ " };
        if path.contains(&m) {
            println!("{cp}{b}{}", ctx.colors.wrap(style::yellow(), "(cycle)"));
        } else {
            render_full_task(ctx, g, m, &cp, b, depth + 1, path);
        }
    }
}

#[allow(dead_code)]
fn _unused(_: &Task) {}
