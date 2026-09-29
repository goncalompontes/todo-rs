use todo_core::{diagnostics, validate};

const USAGE: &str =
    "  check\n    Parse and validate the todo file (syntax, vocab, dates, dependencies).";

pub fn run() {
    todo_plugin::main("check", USAGE, |ctx| {
        let file = ctx.config.file.display().to_string();
        let source = std::fs::read_to_string(&ctx.config.file).unwrap_or_default();
        let done = validate::done_ids(&ctx.done);
        let issues = validate::validate_source(&file, &source, &done, ctx.vocab.as_ref());
        let errors = diagnostics::error_count(&issues);

        if !issues.is_empty() {
            print!("{}", diagnostics::render(&file, &source, &issues));
        }

        let lines = source.lines().count();
        if errors == 0 {
            println!(
                "TODO: check passed ({lines} lines, {} warning(s)).",
                issues.len() - errors
            );
        } else {
            println!("TODO: check failed ({errors} error(s)).");
        }
        Ok(if errors > 0 { 1 } else { 0 })
    });
}
