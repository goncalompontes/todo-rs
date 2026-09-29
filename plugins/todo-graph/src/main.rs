//! graph - render the whole dependency graph (not a tree) in the terminal.
//!
//! Port of the Python `graph` add-on. Graphviz decides the layering (rank) and
//! within-layer order; the nodes are then packed into character cells so boxes
//! never overlap. Coarse dependencies are shown via hub nodes (%group,
//! +module, @kind).
//!
//!     t dep graph [--all] [--width N] [--no-color] [--ascii] [--dep-only]
//!                 [--numbers] [--full] [--image]
//!     t dep graph --dot            # print the Graphviz source
//!     t dep graph --png FILE       # also write a PNG (needs graphviz)
//!     t dep graph --svg FILE       # also write an SVG

use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anstyle::{AnsiColor, Reset};
use todo_core::deps::{DepGraph, State};
use todo_plugin::Context;

const USAGE: &str = "  graph [--all] [--width N] [--no-color] [--ascii] [--dep-only]
        [--numbers] [--full] [--dot] [--png FILE] [--svg FILE] [--image]
    Render the whole dependency graph. Graphviz decides the layering; the nodes
    are packed into character cells so the boxes never overlap.";

fn main() {
    todo_plugin::main("graph", USAGE, run);
}

#[derive(Default)]
struct Opts {
    width: usize,
    no_color: bool,
    ascii: bool,
    dep_only: bool,
    numbers: bool,
    full: bool,
    dot: bool,
    png: Option<String>,
    svg: Option<String>,
    image: bool,
    help: bool,
}

/// A node is either a task (`t<line>`) or a coarse-dependency hub.
struct Node {
    key: String,
    label: String,
    hub: bool,
    state: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Green,
    Cyan,
    Edge,
}

fn run(ctx: &mut Context) -> todo_core::Result<i32> {
    let mut raw = ctx.args.clone();
    if raw.first().map(String::as_str) == Some("graph") {
        raw.remove(0);
    }
    let opts = match parse_args(&raw) {
        Ok(opts) => opts,
        Err(msg) => {
            eprintln!("{msg}");
            return Err(todo_core::Error::External(2));
        }
    };

    let mut out = anstream::stdout().lock();
    if opts.help {
        writeln!(out, "{USAGE}")?;
        return Ok(0);
    }

    let graph = ctx.graph();
    // `--all` is accepted for compatibility but, exactly like the Python
    // original, it is a no-op: dependency-free tasks are included unless
    // `--dep-only` was given.
    let (nodes, edges) = build_graph(&graph, !opts.dep_only);
    if nodes.is_empty() {
        writeln!(out, "no dependency graph (nothing has dependencies)")?;
        return Ok(0);
    }

    let ordered = order_nodes(&nodes);
    let labels_map: HashMap<String, String> = if opts.numbers {
        ordered
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), (i + 1).to_string()))
            .collect()
    } else {
        nodes
            .iter()
            .map(|n| (n.key.clone(), n.label.clone()))
            .collect()
    };
    let dot_src = to_dot(&nodes, &edges, &labels_map);

    if opts.png.is_some() || opts.svg.is_some() {
        let (fmt, target) = match (&opts.png, &opts.svg) {
            (Some(p), _) => ("png", p.as_str()),
            (_, Some(s)) => ("svg", s.as_str()),
            _ => unreachable!(),
        };
        write_dot_image(&dot_src, fmt, target);
        writeln!(out, "wrote {target}")?;
    }

    if opts.dot {
        writeln!(out, "{dot_src}")?;
        return Ok(0);
    }

    let use_color = color_enabled(ctx, &opts);

    if opts.image
        && !opts.ascii
        && which("chafa")
        && let Some(img) = render_chafa(&dot_src, opts.width)
    {
        print_header(&mut out, &nodes, &edges)?;
        writeln!(out, "{img}")?;
        writeln!(out)?;
        print_legend(&mut out, use_color)?;
        if opts.numbers {
            writeln!(out)?;
            print_numbers(&mut out, &ordered, &nodes)?;
        }
        return Ok(0);
    }

    let Some(pos) = positions_from(&dot_src) else {
        eprintln!("graphviz `dot` not available; showing --dot output");
        writeln!(out, "{dot_src}")?;
        return Ok(0);
    };

    print_header(&mut out, &nodes, &edges)?;
    let width = if opts.width > 0 {
        opts.width
    } else {
        term_width()
    };
    let rendered = render(
        &nodes,
        &pos,
        &labels_map,
        &edges,
        width,
        use_color,
        opts.full,
    );
    writeln!(out, "{rendered}")?;
    writeln!(out)?;
    print_legend(&mut out, use_color)?;
    if opts.numbers {
        writeln!(out)?;
        print_numbers(&mut out, &ordered, &nodes)?;
    }
    Ok(0)
}

// --- argument parsing --------------------------------------------------------

fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut opts = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v)),
            _ => (arg.as_str(), None),
        };
        match name {
            "--all" => {}
            "--no-color" => opts.no_color = true,
            "--ascii" => opts.ascii = true,
            "--dep-only" => opts.dep_only = true,
            "--numbers" => opts.numbers = true,
            "--full" => opts.full = true,
            "--dot" => opts.dot = true,
            "--image" => opts.image = true,
            "-h" | "--help" => opts.help = true,
            "--width" => {
                let v = take_value("--width", inline, args, &mut i)?;
                opts.width = v
                    .parse()
                    .map_err(|_| format!("graph: argument --width: invalid int value: '{v}'"))?;
            }
            "--png" => opts.png = Some(take_value("--png", inline, args, &mut i)?),
            "--svg" => opts.svg = Some(take_value("--svg", inline, args, &mut i)?),
            other => return Err(format!("graph: unrecognized arguments: {other}")),
        }
        i += 1;
    }
    Ok(opts)
}

fn take_value(
    name: &str,
    inline: Option<&str>,
    args: &[String],
    i: &mut usize,
) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v.to_string());
    }
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("graph: argument {name}: expected one argument"))
}

// --- graph construction ------------------------------------------------------

struct Builder<'a> {
    graph: &'a DepGraph<'a>,
    members: HashMap<String, Vec<usize>>,
    by_id: HashMap<String, usize>,
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
    edges: HashSet<(String, String)>,
}

impl Builder<'_> {
    fn add_task(&mut self, i: usize) -> String {
        let task = self.graph.all[i];
        let key = format!("t{}", task.line_no);
        if !self.index.contains_key(&key) {
            let label = task
                .id()
                .map(str::to_string)
                .unwrap_or_else(|| format!("#{}", task.line_no));
            let state = state_str(self.graph.state(i));
            self.index.insert(key.clone(), self.nodes.len());
            self.nodes.push(Node {
                key: key.clone(),
                label,
                hub: false,
                state,
            });
        }
        key
    }

    fn add_hub(&mut self, tok: &str) -> String {
        let key = sanitize(tok);
        if !self.index.contains_key(&key) {
            self.index.insert(key.clone(), self.nodes.len());
            self.nodes.push(Node {
                key: key.clone(),
                label: tok.to_string(),
                hub: true,
                state: "",
            });
        }
        key
    }
}

fn build_graph(graph: &DepGraph, include_all: bool) -> (Vec<Node>, Vec<(String, String)>) {
    let mut members: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (i, task) in graph.all.iter().enumerate() {
        for tok in task.tokens() {
            if matches!(tok.chars().next(), Some('+' | '@' | '%')) {
                members.entry(tok.to_string()).or_default().push(i);
            }
        }
        if let Some(id) = task.id() {
            // Python dict comprehension: the last occurrence wins.
            by_id.insert(id.to_string(), i);
        }
    }

    let mut b = Builder {
        graph,
        members,
        by_id,
        nodes: Vec::new(),
        index: HashMap::new(),
        edges: HashSet::new(),
    };

    for (i, task) in graph.all.iter().enumerate() {
        let deps = task.depends();
        if deps.is_empty() {
            if include_all {
                b.add_task(i);
            }
            continue;
        }
        let a = b.add_task(i);
        for target in deps {
            if matches!(target.chars().next(), Some('+' | '@' | '%')) {
                let hub = b.add_hub(target);
                b.edges.insert((a.clone(), hub.clone()));
                let ms = b.members.get(target).cloned().unwrap_or_default();
                for m in ms {
                    if m != i {
                        let member = b.add_task(m);
                        b.edges.insert((hub.clone(), member));
                    }
                }
            } else if let Some(bidx) = b.by_id.get(target).copied() {
                let target_key = b.add_task(bidx);
                b.edges.insert((a.clone(), target_key));
            }
        }
    }

    let mut edges: Vec<(String, String)> = b.edges.into_iter().collect();
    edges.sort();
    (b.nodes, edges)
}

fn sanitize(tok: &str) -> String {
    let mut out = String::from("h");
    for c in tok.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

fn state_str(state: State) -> &'static str {
    match state {
        State::Blocked => "blocked",
        State::Ready => "ready",
        State::Done => "done",
    }
}

fn order_nodes(nodes: &[Node]) -> Vec<String> {
    let mut idx: Vec<usize> = (0..nodes.len()).collect();
    idx.sort_by_key(|&i| nodes[i].label.to_lowercase());
    idx.into_iter().map(|i| nodes[i].key.clone()).collect()
}

// --- Graphviz ---------------------------------------------------------------

fn to_dot(nodes: &[Node], edges: &[(String, String)], labels: &HashMap<String, String>) -> String {
    let mut out = String::new();
    out.push_str("digraph deps {\n");
    out.push_str("  rankdir=TB;\n");
    out.push_str("  bgcolor=\"transparent\";\n");
    out.push_str("  node [shape=box, style=rounded, margin=0.06, fontname=\"Helvetica\", fontsize=11, penwidth=1.4];\n");
    out.push_str("  edge [color=\"#888888\", arrowsize=0.7];\n");
    for node in nodes {
        let name = labels
            .get(&node.key)
            .map(String::as_str)
            .unwrap_or(&node.label);
        let color = match node.state {
            "blocked" => "#cc3333",
            "ready" => "#2f8f2f",
            _ => "#2277bb",
        };
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\", color=\"{}\", fontcolor=\"{}\"];\n",
            node.key,
            dot_escape(name),
            color,
            color
        ));
    }
    for (a, b) in edges {
        out.push_str(&format!("  \"{a}\" -> \"{b}\";\n"));
    }
    out.push('}');
    out
}

fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn positions_from(dot_src: &str) -> Option<HashMap<String, (f64, f64)>> {
    let output = run_with_input("dot", &["-Tplain"], dot_src.as_bytes())?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut pos = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.first() == Some(&"node")
            && parts.len() >= 4
            && let (Ok(x), Ok(y)) = (parts[2].parse::<f64>(), parts[3].parse::<f64>())
        {
            pos.insert(parts[1].to_string(), (x, y));
        }
    }
    Some(pos)
}

fn run_with_input(prog: &str, args: &[&str], input: &[u8]) -> Option<std::process::Output> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    child.wait_with_output().ok()
}

fn write_dot_image(dot_src: &str, fmt: &str, target: &str) {
    let mut child = match Command::new("dot")
        .arg(format!("-T{fmt}"))
        .arg("-o")
        .arg(target)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(dot_src.as_bytes());
    }
    let _ = child.wait();
}

// --- chafa image rendering ---------------------------------------------------

fn render_chafa(dot_src: &str, width: usize) -> Option<String> {
    if !which("chafa") {
        return None;
    }
    let path = temp_png_path();
    let path_str = path.to_str()?;
    let output = run_with_input("dot", &["-Tpng", "-o", path_str], dot_src.as_bytes());
    let Some(output) = output else {
        let _ = std::fs::remove_file(&path);
        return None;
    };
    if !output.status.success() {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    let mut cmd = Command::new("chafa");
    cmd.arg("--animate=off");
    if width > 0 {
        cmd.arg(format!("--size={width}x"));
    }
    cmd.arg(&path);
    let result = cmd.output();
    let _ = std::fs::remove_file(&path);
    let result = result.ok()?;
    if result.status.success() {
        let text = String::from_utf8_lossy(&result.stdout);
        let trimmed = text.trim_end_matches('\n').to_string();
        if !trimmed.trim().is_empty() {
            return Some(trimmed);
        }
    }
    None
}

fn temp_png_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "todo-graph-{}-{}-{}.png",
        std::process::id(),
        nanos,
        n
    ))
}

fn which(prog: &str) -> bool {
    if prog.contains('/') {
        return Path::new(prog).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(prog).is_file())
}

// --- character-cell renderer -------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render(
    nodes: &[Node],
    pos: &HashMap<String, (f64, f64)>,
    labels: &HashMap<String, String>,
    edges: &[(String, String)],
    width: usize,
    color: bool,
    full: bool,
) -> String {
    let tenths = |y: f64| -> i64 { (y * 10.0).round() as i64 };

    let mut ys: Vec<i64> = pos.values().map(|&(_, y)| tenths(y)).collect();
    ys.sort_unstable();
    ys.dedup();
    ys.reverse();
    let rank: HashMap<i64, usize> = ys.iter().enumerate().map(|(i, &y)| (y, i)).collect();

    let mut layers: HashMap<usize, Vec<String>> = HashMap::new();
    for node in nodes {
        if let Some(&(_, y)) = pos.get(&node.key) {
            let r = rank[&tenths(y)];
            layers.entry(r).or_default().push(node.key.clone());
        }
    }
    for ks in layers.values_mut() {
        ks.sort_by(|a, b| {
            let ax = pos.get(a).map(|p| p.0).unwrap_or(0.0);
            let bx = pos.get(b).map(|p| p.0).unwrap_or(0.0);
            ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    const RPL: usize = 6;
    const GAP: usize = 3;
    let nlayers = ys.len();
    let cw = width.max(24);

    let mut lab: HashMap<String, String> = HashMap::new();
    for node in nodes {
        let s = labels
            .get(&node.key)
            .cloned()
            .unwrap_or_else(|| node.key.clone());
        let value = if full {
            s
        } else {
            trunc(&s, cw.saturating_sub(4).max(3))
        };
        lab.insert(node.key.clone(), value);
    }
    let mut w: HashMap<String, usize> = HashMap::new();
    for node in nodes {
        let lw = lab.get(&node.key).map(|s| s.chars().count()).unwrap_or(0) + 2;
        w.insert(node.key.clone(), lw.max(4));
    }

    let avail = cw.saturating_sub(2);
    let mut slots: Vec<Vec<String>> = Vec::new();
    for r in 0..nlayers {
        let ks = layers.get(&r).cloned().unwrap_or_default();
        let mut cur: Vec<String> = Vec::new();
        let mut curw = 0usize;
        for k in ks {
            let kw = w.get(&k).copied().unwrap_or(4);
            let add = kw + if cur.is_empty() { 0 } else { GAP };
            if !cur.is_empty() && curw + add > avail {
                slots.push(std::mem::take(&mut cur));
                curw = 0;
                cur.push(k);
                curw += kw;
            } else {
                cur.push(k);
                curw += add;
            }
        }
        if !cur.is_empty() {
            slots.push(cur);
        }
    }

    let ch = (slots.len() * RPL + 3).max(5);
    let mut grid: Vec<Vec<char>> = vec![vec![' '; cw]; ch];
    let mut colr: Vec<Vec<Option<Color>>> = vec![vec![None; cw]; ch];

    let mut cell: HashMap<String, (usize, usize)> = HashMap::new();
    for (si, ks) in slots.iter().enumerate() {
        let lw: usize = ks
            .iter()
            .map(|k| w.get(k).copied().unwrap_or(4))
            .sum::<usize>()
            + GAP * (ks.len() - 1);
        let mut x = cw.saturating_sub(lw) / 2;
        let y = si * RPL + 1;
        for k in ks {
            cell.insert(k.clone(), (x, y));
            x += w.get(k).copied().unwrap_or(4) + GAP;
        }
    }

    let utf8 = is_utf8();
    let (down, up) = if utf8 { ('▼', '▲') } else { ('v', '^') };

    let mut lanes: HashMap<usize, usize> = HashMap::new();
    for (a, b) in edges {
        let (Some(&(ax, ay)), Some(&(bx, by))) = (cell.get(a), cell.get(b)) else {
            continue;
        };
        let acx = ax + w.get(a).copied().unwrap_or(4) / 2;
        let bcx = bx + w.get(b).copied().unwrap_or(4) / 2;
        let abot = ay + 3;
        let btop = by as isize - 1;
        if by > ay {
            let c = lanes.get(&by).copied().unwrap_or(0);
            lanes.insert(by, c + 1);
            let lane = abot.max(by.saturating_sub(2 + (c % 2)));
            vline(
                &mut grid,
                &mut colr,
                acx as isize,
                abot as isize,
                lane as isize,
                Some(Color::Edge),
            );
            hline(
                &mut grid,
                &mut colr,
                acx as isize,
                bcx as isize,
                lane as isize,
                Some(Color::Edge),
            );
            vline(
                &mut grid,
                &mut colr,
                bcx as isize,
                lane as isize,
                btop,
                Some(Color::Edge),
            );
            if acx != bcx {
                let left = if bcx > acx { '└' } else { '┘' };
                let right = if bcx > acx { '┐' } else { '┌' };
                put(
                    &mut grid,
                    &mut colr,
                    acx as isize,
                    lane as isize,
                    left,
                    Some(Color::Edge),
                );
                put(
                    &mut grid,
                    &mut colr,
                    bcx as isize,
                    lane as isize,
                    right,
                    Some(Color::Edge),
                );
            }
            put(
                &mut grid,
                &mut colr,
                bcx as isize,
                btop,
                down,
                Some(Color::Edge),
            );
        } else {
            hline(
                &mut grid,
                &mut colr,
                acx as isize,
                bcx as isize,
                abot as isize,
                Some(Color::Edge),
            );
            let glyph = if by < ay { up } else { down };
            put(
                &mut grid,
                &mut colr,
                bcx as isize,
                btop,
                glyph,
                Some(Color::Edge),
            );
        }
    }

    let mut state_by_key: HashMap<&str, &str> = HashMap::new();
    for node in nodes {
        state_by_key.insert(node.key.as_str(), node.state);
    }

    for (k, &(x, y)) in &cell {
        let ww = w.get(k).copied().unwrap_or(4);
        let tag = if color {
            match state_by_key.get(k.as_str()).copied().unwrap_or("") {
                "blocked" => Some(Color::Red),
                "ready" => Some(Color::Green),
                _ => Some(Color::Cyan),
            }
        } else {
            None
        };
        let label = lab
            .get(k)
            .map(String::as_str)
            .or_else(|| labels.get(k).map(String::as_str))
            .unwrap_or(k.as_str());
        for i in 0..ww {
            put(&mut grid, &mut colr, (x + i) as isize, y as isize, '─', tag);
            put(
                &mut grid,
                &mut colr,
                (x + i) as isize,
                (y + 2) as isize,
                '─',
                tag,
            );
        }
        for j in 0..3 {
            put(&mut grid, &mut colr, x as isize, (y + j) as isize, '│', tag);
            put(
                &mut grid,
                &mut colr,
                (x + ww - 1) as isize,
                (y + j) as isize,
                '│',
                tag,
            );
        }
        put(&mut grid, &mut colr, x as isize, y as isize, '┌', tag);
        put(
            &mut grid,
            &mut colr,
            (x + ww - 1) as isize,
            y as isize,
            '┐',
            tag,
        );
        put(&mut grid, &mut colr, x as isize, (y + 2) as isize, '└', tag);
        put(
            &mut grid,
            &mut colr,
            (x + ww - 1) as isize,
            (y + 2) as isize,
            '┘',
            tag,
        );
        for (i, glyph) in label.chars().enumerate() {
            put(
                &mut grid,
                &mut colr,
                (x + 1 + i) as isize,
                (y + 1) as isize,
                glyph,
                tag,
            );
        }
    }

    let rows_with: Vec<usize> = (0..grid.len())
        .filter(|&i| grid[i].iter().any(|&c| c != ' '))
        .collect();
    if let (Some(&lo), Some(&hi)) = (rows_with.first(), rows_with.last()) {
        grid = grid[lo..=hi].to_vec();
        colr = colr[lo..=hi].to_vec();
    }

    if !color {
        return grid
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut lines = Vec::with_capacity(grid.len());
    for (ri, row) in grid.iter().enumerate() {
        let mut line = String::new();
        let mut cur: Option<Color> = None;
        for (x, &glyph) in row.iter().enumerate() {
            let c = colr[ri][x];
            if c != cur {
                line.push_str(&color_code(c));
                cur = c;
            }
            line.push(glyph);
        }
        lines.push(format!("{}{}", line.trim_end(), Reset));
    }
    lines.join("\n").trim_matches('\n').to_string()
}

fn put(
    grid: &mut [Vec<char>],
    colr: &mut [Vec<Option<Color>>],
    x: isize,
    y: isize,
    glyph: char,
    tag: Option<Color>,
) {
    let ch = grid.len() as isize;
    let cw = if ch > 0 { grid[0].len() as isize } else { 0 };
    if y >= 0 && y < ch && x >= 0 && x < cw {
        grid[y as usize][x as usize] = glyph;
        colr[y as usize][x as usize] = tag;
    }
}

fn vline(
    grid: &mut [Vec<char>],
    colr: &mut [Vec<Option<Color>>],
    x: isize,
    y0: isize,
    y1: isize,
    tag: Option<Color>,
) {
    let (lo, hi) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    for y in lo..=hi {
        if y < 0 || y >= grid.len() as isize {
            continue;
        }
        if x < 0 || x >= grid[0].len() as isize {
            continue;
        }
        if grid[y as usize][x as usize] == ' ' {
            put(grid, colr, x, y, '│', tag);
        }
    }
}

fn hline(
    grid: &mut [Vec<char>],
    colr: &mut [Vec<Option<Color>>],
    x0: isize,
    x1: isize,
    y: isize,
    tag: Option<Color>,
) {
    let (lo, hi) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    for x in lo..=hi {
        if y < 0 || y >= grid.len() as isize {
            continue;
        }
        if x < 0 || x >= grid[0].len() as isize {
            continue;
        }
        if grid[y as usize][x as usize] == ' ' {
            put(grid, colr, x, y, '─', tag);
        }
    }
}

// --- headers, legend, numbers ------------------------------------------------

fn print_header(
    out: &mut impl Write,
    nodes: &[Node],
    edges: &[(String, String)],
) -> std::io::Result<()> {
    let blocked = nodes.iter().filter(|n| n.state == "blocked").count();
    let ready = nodes.iter().filter(|n| n.state == "ready").count();
    let groups = nodes.iter().filter(|n| n.hub).count();
    writeln!(
        out,
        "Dependency graph - {} nodes, {} edges · {} blocked, {} ready, {} groups",
        nodes.len(),
        edges.len(),
        blocked,
        ready,
        groups
    )?;
    writeln!(out)
}

fn print_legend(out: &mut impl Write, color: bool) -> std::io::Result<()> {
    let (red, green, cyan, edge, reset) = if color {
        (
            AnsiColor::Red.render_fg().to_string(),
            AnsiColor::Green.render_fg().to_string(),
            AnsiColor::Cyan.render_fg().to_string(),
            AnsiColor::BrightBlack.render_fg().to_string(),
            Reset.to_string(),
        )
    } else {
        Default::default()
    };
    writeln!(out, "Legend:")?;
    writeln!(
        out,
        "  {red}■{reset} blocked    - has an unfinished dependency"
    )?;
    writeln!(out, "  {green}■{reset} ready      - no open dependencies")?;
    writeln!(
        out,
        "  {cyan}■{reset} group      - %group, +module or @kind"
    )?;
    writeln!(
        out,
        "  {edge}─▶{reset} depends on - A ──▶ B means \"A depends on B\""
    )?;
    writeln!(out, "  {edge}boxes = tasks (by name) and groups{reset}")?;
    Ok(())
}

fn print_numbers(out: &mut impl Write, ordered: &[String], nodes: &[Node]) -> std::io::Result<()> {
    let by_key: HashMap<&str, &Node> = nodes.iter().map(|n| (n.key.as_str(), n)).collect();
    for (i, key) in ordered.iter().enumerate() {
        let Some(node) = by_key.get(key.as_str()) else {
            continue;
        };
        let tag = if node.state.is_empty() {
            String::new()
        } else {
            format!("  [{}]", node.state)
        };
        writeln!(out, " {:>2}  {}{}", i + 1, node.label, tag)?;
    }
    Ok(())
}

// --- helpers -----------------------------------------------------------------

fn color_code(c: Option<Color>) -> String {
    match c {
        Some(Color::Red) => AnsiColor::Red.render_fg().to_string(),
        Some(Color::Green) => AnsiColor::Green.render_fg().to_string(),
        Some(Color::Cyan) => AnsiColor::Cyan.render_fg().to_string(),
        Some(Color::Edge) => AnsiColor::BrightBlack.render_fg().to_string(),
        None => Reset.to_string(),
    }
}

fn color_enabled(ctx: &Context, opts: &Opts) -> bool {
    if opts.no_color {
        return false;
    }
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    if std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false) {
        return false;
    }
    ctx.colors.enabled() && std::io::stdout().is_terminal()
}

fn is_utf8() -> bool {
    let loc = std::env::var("LC_ALL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("LC_CTYPE").ok().filter(|s| !s.is_empty()))
        .or_else(|| std::env::var("LANG").ok().filter(|s| !s.is_empty()))
        .unwrap_or_default();
    loc.to_lowercase().contains("utf")
}

fn trunc(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_string();
    }
    let ell: &[char] = if is_utf8() { &['…'] } else { &['.', '.'] };
    if n <= ell.len() {
        return chars[..n].iter().collect();
    }
    let mut out: String = chars[..n - ell.len()].iter().collect();
    out.extend(ell);
    out
}

fn term_width() -> usize {
    if let Ok(file) = std::fs::File::open("/dev/tty")
        && let Ok(output) = Command::new("stty")
            .arg("size")
            .stdin(Stdio::from(file))
            .output()
        && output.status.success()
        && let Ok(text) = String::from_utf8(output.stdout)
        && let Some(cols) = text
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse::<usize>().ok())
    {
        return cols;
    }
    if let Ok(cols) = std::env::var("COLUMNS")
        && let Ok(n) = cols.trim().parse::<usize>()
    {
        return n;
    }
    100
}
