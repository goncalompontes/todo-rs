# Writing a plugin

A plugin is an ordinary executable that the host dispatches to when an action
name matches it. The `todo-plugin` crate removes the boilerplate.

## 1. Create the crate

```
cargo new --bin plugins/todo-thing
```

`plugins/todo-thing/Cargo.toml`:

```toml
[package]
name = "todo-thing"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "thing"            # the action name the host dispatches
path = "src/main.rs"

[dependencies]
todo-core = { path = "../../crates/todo-core" }
todo-plugin = { path = "../../crates/todo-plugin" }
```

Add `"plugins/todo-thing"` to the workspace `members` in the root `Cargo.toml`.

## 2. Implement it

```rust
use todo_plugin::prelude::*;

struct Thing;

impl Plugin for Thing {
    fn name(&self) -> &str {
        "thing"
    }

    fn usage(&self) -> &str {
        "  thing [--all] [TERM...]\n    Do the thing."
    }

    fn run(&self, ctx: &mut Context) -> Result<i32> {
        // Uniform argument parsing.
        let args = Args::parse(ctx.args(), &["all"], &["width"])?;
        let width: Option<usize> = args.value("width")?;

        // Shared styling, colour policy already applied.
        for task in ctx.store.open_tasks() {
            let style = if task.done { style::dim() } else { style::bold() };
            println!("{}", ctx.colors.paint(style, format!("#{}", task.line_no)));
        }
        let _ = (args.has("all"), width);
        Ok(0)
    }
}

fn main() {
    todo_plugin::run_plugin(Thing);
}
```

For a quick one-off, use the closure form:

```rust
fn main() {
    todo_plugin::main("thing", "  thing", |ctx| {
        println!("{} tasks", ctx.store.tasks().count());
        Ok(0)
    });
}
```

## 3. Install it

```
./install.sh
```

`install.sh` auto-discovers every executable in `target/release`, so a new
plugin needs no changes to the script. Add aliases there only if one binary
serves several action names.

## What `Context` gives you

| Field / method | Purpose |
|---|---|
| `config` | `Config` (paths, flags, env) |
| `store` | the parsed `todo.txt` (`.tasks()`, `.open_tasks()`, mutate + `.save()`) |
| `done` | tasks from `done.txt` |
| `vocab` / `vocab()` | parsed `.todo/vocab` |
| `args` / `args()` | raw arguments (use `Args::parse`) |
| `colors` | shared `Palette` (anstyle-based) |
| `graph()` | `DepGraph` — blocking, readiness, roots, `next` ranking |
| `checker()` | parser + dependency validation |

## Shared facilities

- **Parsing / validation**: `todo_core::parse`, `todo_core::validate`,
  `todo_core::diagnostics` (ariadne reports).
- **Styling**: `todo_core::style` (`Palette`, `style::bold/dim/red/...`).
- **Dates**: `todo_core::date` (`today`, `parse`, `add_months`, ...).
- **Links**: `todo_core::paths` (`parse`, `build_url`, `osc8`).
- **HTTP/commands**: `std::process`; prefer a crate when one exists.
