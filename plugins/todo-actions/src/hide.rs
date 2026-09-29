use todo_core::task::Task;

const USAGE: &str = "  hide [TERM...]\n    List tasks containing NONE of the given TERM(s).";

pub fn run() {
    todo_plugin::main("hide", USAGE, |ctx| {
        let terms: Vec<String> = ctx.args.iter().map(|t| t.to_ascii_lowercase()).collect();
        let selected: Vec<&Task> = ctx
            .store
            .open_tasks()
            .filter(|task| {
                if terms.is_empty() {
                    return true;
                }
                let hay = task.render().to_ascii_lowercase();
                !terms.iter().any(|term| hay.contains(term))
            })
            .collect();
        crate::print_numbered(ctx, &selected, false);
        Ok(0)
    });
}
