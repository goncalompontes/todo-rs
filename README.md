# todo-rs

[![CI](https://github.com/goncalompontes/todo-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/goncalompontes/todo-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A backwards-compatible `todo.sh` host with a plugin system, written in Rust.

A Rust reimplementation of the `todo.txt` CLI and its add-on/plugin system.
It is designed as a drop-in replacement for `todo.sh`:

- It reads the same `todo.txt`, `done.txt` and `.todo/config` files.
- It accepts the same global options (`-d`, `-p`, `-c`, `-@`, `-+`, `-a`, `-A`,
  `-t`, `-T`, `-n`, `-N`, `-P`, `-v`, `-V`, `-x`, `-f`, `-h`).
- It implements the `todo.sh` built-ins (`add`, `do`, `pri`, `list*`, `archive`,
  `replace`, `prepend`, `append`, `move`, `del`, `deduplicate`, `report`, ...).
- It dispatches any other action to the same action directories `todo.sh` uses.

## Workspace layout

```
crates/todo-core     shared engine: config, task model, store, dependency
                     graph, validation, paths/links, plugin dispatch
crates/todo-plugin   SDK for writing native Rust plugins
crates/todo          the `todo` host binary (todo.sh core + plugin runner)
plugins/todo-actions native ports of the add-ons, as separate plugin binaries
```

## Plugin system

Two kinds of action are supported and intermix freely:

1. **Legacy add-ons** — executables in the action directories
   (`~/.config/todo/actions`, `~/.todo/actions`, `~/.todo.actions.d`, or
   `$TODO_ACTIONS_DIR`). They inherit the usual `TODO_FILE`,
   `DONE_FILE`, `TODO_VOCAB_FILE`, `TODO_FULL_SH`, … environment and are
   invoked as `ACTION [args...]`. They may implement a `usage` subcommand.
2. **Native Rust plugins** — built against `todo-plugin`, installed under
   `$TODO_RUST_ACTIONS_DIR` (default `~/.config/todo/rust-actions`). This
   directory is searched first, so a ported plugin shadows its legacy
   namesake without touching it.

A minimal plugin:

```rust
fn main() {
    todo_plugin::main("my-action", "  my-action [args]", |ctx| {
        for task in ctx.store.open_tasks() {
            println!("{}", task.render());
        }
        Ok(0)
    });
}
```

`ctx` exposes the config, store, `done.txt`, vocab, shared style, arguments, and
a `DepGraph` with blocking/readiness/ranking. `todo_plugin::invoked_name()`
lets one binary serve several action names (`dep`/`blocked`/`ready`/`next`).

For structured plugins use the `Plugin` trait plus the uniform `Args` parser
and the `prelude`:

```rust
use todo_plugin::prelude::*;

struct Hello;
impl Plugin for Hello {
    fn name(&self) -> &str { "hello" }
    fn usage(&self) -> &str { "  hello" }
    fn run(&self, ctx: &mut Context) -> Result<i32> {
        let args = Args::parse(ctx.args(), &["all"], &[])?;
        let _ = args.has("all");
        Ok(0)
    }
}
fn main() { todo_plugin::run_plugin(Hello); }
```

See [`plugins/README.md`](plugins/README.md) for a step-by-step template.

## Parsing & validation

Validation is parser-driven. `todo-core::parse` uses **chumsky** to parse each
todo.txt line into a `ParsedLine` with span-annotated tokens (project, context,
group, `key:value`), checking the syntax (done marker, priority, dates) as it
goes. `todo-core::validate` layers the semantic checks on top:

- metadata value checks from the parser (`due:`/`t:` dates, `sev:` numbers,
  `id:`/`depends:`/`p:` shapes, `file:`/`dir:`/`path:` locations),
- vocab checks for `+module` / `@kind` / `status:`,
- duplicate and self ids,
- dangling `depends:` ids, and **recursive dependency cycles**.

Diagnostics are rendered with **ariadne** (coloured, span-labelled source
snippets). `todo check` and `todo dep validate` both use this pipeline.

## Design & libraries

One facility per job, shared from `todo-core` so the host and every plugin look
and behave the same:

| Concern | Crate | Surface |
|---|---|---|
| CLI | `clap` (+ `shlex`) | `crates/todo` (external subcommands → plugins) |
| Parsing | `chumsky` | `todo_core::parse` (zero-copy) |
| Diagnostics | `ariadne` | `todo_core::diagnostics`, `validate` |
| Parallelism | `rayon` | parallel line parsing / validation / store load |
| Async | `tokio` + `futures` | `todo-plugin` `async` feature; board event loop |
| Colour | `anstyle` | `todo_core::style` (`Palette`, `style::*`) |
| Links | `terminal-link` | `todo_core::paths::osc8` |
| Dates | `chrono` | `todo_core::date` |
| Path expansion | `shellexpand` | `todo_core::config` |
| TUI | `ratatui` + `crossterm` | `plugins/todo-board` |

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the design and the
extension points (new plugins, async/parallel workloads, borrowed read models).

`Palette::detect` applies one colour policy everywhere (config `plain`,
`NO_COLOR`, `TERM=dumb`, non-TTY). New plugins depend only on `todo-plugin`
and get all of the above through `todo_plugin::prelude`.

## Install

```sh
git clone https://github.com/goncalompontes/todo-rs
cd todo-rs

./install.sh                             # builds --release, links plugins
TODO_BIN_DIR=~/.local/bin ./install.sh   # also link the host there
# then point `t` at ~/.local/bin/todo (see below)
```

`install.sh` auto-discovers every executable in `target/release`, so a new
plugin crate needs no change to the script.

To use `t` from any repo, add a function like:

```sh
t() {
  local dir=$PWD found
  while [[ -n $dir ]]; do
    [[ -f $dir/.todo/config ]] && { found=$dir/.todo/config; break; }
    [[ $dir == / ]] && break
    dir=${dir:h}
  done
  command todo ${found:+-d "$found"} "$@"
}
```

The host can also be installed directly with `cargo install --path crates/todo`.

## License

MIT — see [LICENSE](LICENSE).

## Porting status

Every add-on has been ported to a native Rust plugin; the legacy scripts are
no longer needed (but still work if present, since the host dispatches to any
executable and the rust plugins simply take priority).

| Action | Crate |
|---|---|
| `add`, `do`, `pri`, `list*`, `archive`, `append`, `prepend`, `replace`, `move`, `del`, `deduplicate`, `report`, help | host (`crates/todo`) |
| `check`, `vocab`, `hide`, `finish`, `here`, `projectview`, `contextview` | `plugins/todo-actions` |
| `dep`, `blocked`, `ready`, `next` (tree/graph/validate/add/rm/id/sync) | `plugins/todo-actions` |
| `graph` | `plugins/todo-graph` (petgraph + Graphviz `-Tplain`, DOT/PNG/SVG, chafa) |
| `board` | `plugins/todo-board` (ratatui + crossterm, `--snapshot`) |
| `due` | `plugins/todo-due` |
| `again` | `plugins/todo-again` |
| `open`, `editor-open` | `plugins/todo-open` |
| `link`, `link-open` | `plugins/todo-link` |
