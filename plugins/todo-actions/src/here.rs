use todo_core::paths;
use todo_core::task::Task;

const USAGE: &str =
    "  here [PATH]\n    Show open todos pertaining to a file or directory (default: cwd).";

pub fn run() {
    todo_plugin::main("here", USAGE, |ctx| {
        let mut target = String::new();
        let mut args = ctx.args.iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--path" => {
                    if let Some(v) = args.next() {
                        target = v.clone();
                    }
                }
                _ => target = arg.clone(),
            }
        }

        let base = ctx
            .config
            .file
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| ctx.config.dir.clone());
        if target.is_empty() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| base.clone());
            target = paths::cwd_rel(&cwd, &base);
        }

        let selected: Vec<&Task> = ctx
            .store
            .open_tasks()
            .filter(|t| paths::line_matches_path(t.tokens(), &target, &base))
            .collect();

        for task in &selected {
            let line = crate::linkify_line(ctx, task);
            println!("{} {}", task.line_no, line);
        }
        if selected.is_empty() {
            println!("TODO: no todos for {target}");
        } else {
            println!("--");
            println!(
                "TODO: {} todo(s) for {target}  (open with: t open N)",
                selected.len()
            );
        }
        Ok(0)
    });
}
