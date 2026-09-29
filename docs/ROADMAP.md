# Roadmap — expansion points

Status legend: **done** (implemented this cycle) · **planned** (designed, not yet
implemented).

## 1. Structured output (`serde` + `serde_json`) — done

- `todo-core` gains `TaskJson` (`serde::Serialize`) plus `Task::to_json()`.
- `todo --json` emits a JSON array for `list`, `listall`, `listpri` and a JSON
  object for `report`.
- `dep --json` emits structured output for `ready`, `blocked`, `next` and
  `validate`.
- Planned: `--format table|tsv|json` as a generic selector, and JSON for
  `board --snapshot`.

## 2. Shell integration (`clap_complete`, `clap_mangen`) — done

- `todo completions <shell>` generates completions from the same `clap`
  definition (bash/zsh/fish/powershell/elvish).
- `todo man [--out DIR]` renders a roff man page.

## 3. Native config (`serde` + `toml`) — done

- `.todo/config.toml` (or `$TODO_CONFIG_TOML`, or `$XDG_CONFIG_HOME/todo/config.toml`)
  is parsed and layered **over** the bash config, so teams can use a typed file
  while legacy configs keep working.
- All path fields expand `~`; unknown keys are rejected with a helpful error.

## 4. Pretty tables (`comfy-table`) — done

- `report` renders its per-project breakdown as a table on a terminal (plain
  columns when piped or `--plain`).

## 5. Search & filtering (`regex`, `memchr`, `fuzzy-matcher`) — done (regex) / planned

- `list`/`listall`/`listpri` accept `--regex` so filters are regular
  expressions; default stays the legacy case-insensitive substring match.
- Planned: interactive fuzzy filter in `board` (`fuzzy-matcher`/`nucleo`) and
  `memchr`-backed token scans for very large files.

## 6. Borrowed read model (`Document<'a>` / `DocTask<'a>`) — done (module)

- `todo-core::read` exposes a zero-copy, read-only view over a source buffer:
  `Document::parse(&text)` yields `DocTask<'a>` whose tokens, tags, description
  and JSON view borrow the text. Covered by unit tests; available to plugins.
  The owned `Store` remains for mutation.
- Planned: migrate the host's read-only commands (`list`, `report`) onto
  `Document` so even the owned `Task` allocations disappear for reads, and
  expose it on `Context`.

## 7. Testing — done

- `proptest`: parser never panics and round-trips arbitrary lines.
- `assert_cmd` + `predicates` + `tempfile`: end-to-end CLI tests
  (`add`/`list`/`do`/`check`) against a temp config.

## 8. Async / remote backends (`reqwest`, `notify`) — planned

- Design: a `todo sync` command plus a `watcher` plugin built on `main_async`.
  `reqwest` (rustls) pulls/pushes `todo.txt` against a URL; `notify` watches the
  file and re-renders the board on change.
- Reuses `Context::load_async`; no core changes required beyond an optional
  `remote { url, token }` config section.

## 9. Dynamic plugins (`libloading` cdylib / `wasmtime`) — planned

- Design: a stable C ABI in `todo-plugin` (`extern "C" fn todo_plugin_run(ctx)`)
  loaded from `~/.config/todo/plugins/*.so` ahead of executable dispatch; WASM
  as a sandboxed alternative. Needs versioned ABI + capability boundary.

## 10. Rich terminal (`syntect`, `comfy-table`) — partial

- Tables done (see 4). Planned: `syntect`-highlighted file preview for `open`.

## 11. More output shapes — planned

- `csv` writer (`csv` crate) for spreadsheets; `--json` streaming for large
  files.

## Compatibility contract

Every addition is **opt-in**: the default output stays byte-compatible with the
legacy scripts (verified by the `dep`/`board` diff tests). New flags (`--json`,
`--regex`, …) never alter the default path.
