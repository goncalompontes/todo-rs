# Architecture & extension points

## Layers

```
crates/todo-core     engine: config, task/store, dependency graph, parser,
                     diagnostics, validation, style, paths, plugin dispatch
crates/todo-plugin   plugin SDK: Context, Plugin trait, Args, prelude, async entry
crates/todo          host: clap CLI + todo.sh built-ins + plugin runner
plugins/*            one crate per action (or a bundle like todo-actions)
```

The host and every plugin share `todo-core`, so behaviour (colour policy,
parsing, dependency semantics, output formatting) is consistent by construction.

## Design decisions

### CLI: `clap` with external subcommands
The host defines the `todo.sh` built-ins as `clap` subcommands for real
validation and help, and uses `#[command(external_subcommand)]` to forward any
unknown action (with all of its arguments, including `--flags`) to the plugin
system. This keeps `todo.sh` compatibility while giving typed option parsing.

### Zero-copy parsing
`todo_core::parse` is `chumsky`-based and **borrows** from the source line:
`ParsedLine<'a>` / `ParsedToken<'a>` / `TokenKind<'a>` hold `&'a str`. Parsing a
file allocates only token vectors and diagnostics, not the text. Validation
consumes those borrowed views directly.

`Task`/`Store` remain owned (they support mutation and `.save()`), but the
read-only pipeline (parse → validate → dependency graph) never copies strings.

### Parallelism: `rayon`
Line parsing is data-parallel and lock-free because the parser is zero-copy and
`Sync`:
- `Store::from_text` parses lines with `par_iter`;
- `validate::validate_source` parses lines with `par_iter` then folds results.

CPU-bound, no shared mutable state — a natural fit.

### Async: `tokio`
Async is opt-in via `todo-plugin`'s `async` feature:
- `Context::load_async` reads `todo.txt`, `done.txt` and the vocab file
  concurrently;
- `main_async` builds a current-thread runtime;
- the `board` TUI drives a `crossterm::event::EventStream` instead of blocking
  `event::read()`, so input, resize and (future) timers share one loop.

Async is deliberately *not* used for the short-lived, local, mostly CPU-bound
commands (`list`, `dep`, `check`, …): a runtime there would add overhead and
complexity for no concurrency to exploit. It becomes valuable for network or
watch workloads (see below).

### Errors & diagnostics
`ariadne` renders parser/validation issues with spans. The parser is the primary
validator; semantic checks (vocab, dependency existence, cycles) layer on top.

## Adding a plugin

See [`../plugins/README.md`](../plugins/README.md). In short: create a crate with
a `[[bin]]` named after the action, depend on `todo-plugin`, and implement
`Plugin` (or use the closure/async entry). `install.sh` auto-discovers new
binaries.

## Where to extend next

- **Structured output**: add `serde` + `serde_json` (and maybe `csv`) behind a
  `--json`/`--format` flag for `list`/`next`/`report`, so other tools and the
  agent can consume results without scraping ANSI.
- **Async/remote backends**: `reqwest` for syncing a todo.txt to a server, or
  `notify` for a file watcher — reuse `main_async` and `Context::load_async`.
- **Search & filtering**: `regex` or `fuzzy-matcher`/`nucleo` for interactive
  filtering in the board and `list`; `memchr` for fast token scans.
- **Shell integration**: `clap_complete` and `clap_mangen` to emit completions
  and a man page from the same `clap` definition.
- **Rich terminal output**: `comfy-table` for `report`, `syntect` for syntax
  highlighting in `open`.
- **More input formats**: `serde` + `toml`/`json` for a native config as an
  alternative to sourcing `todo.sh` bash configs.
- **Borrowed read model**: introduce a `Document<'a>` that yields `TaskRef<'a>`
  for the read-only commands, extending the zero-copy approach from the parser
  to the store itself (keeps the owned `Store` for mutation).
- **Dynamic plugins**: `libloading` (cdylib) or `wasmtime` (wasm) for in-process
  Rust/WASM plugins, complementing the current executable dispatch.
- **Testing**: `insta` snapshots for command output, `assert_cmd`/`predicates`
  for CLI integration tests, `proptest` for the parser.

## Compatibility contract

The external behaviour is pinned to the legacy scripts: `dep`/`board` output is
byte-identical (piped, no colour), config files are sourced through bash, and
unknown actions still dispatch to legacy add-ons. Any refactor must keep the
`dep`/`board` diff tests green.
