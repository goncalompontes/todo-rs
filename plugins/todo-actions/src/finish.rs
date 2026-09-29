use todo_core::date;
use todo_core::task::Task;

const USAGE: &str = "  finish <id|#n> ...\n    Mark tasks done by stable id or line number.";

pub fn run() {
    todo_plugin::main("finish", USAGE, |ctx| {
        let mut dry = false;
        let mut selectors = Vec::new();
        for arg in &ctx.args {
            match arg.as_str() {
                "--dry-run" => dry = true,
                other => selectors.push(other.trim_start_matches('#').to_string()),
            }
        }
        if selectors.is_empty() {
            eprintln!("TODO: usage: todo finish <id|#n> ...");
            return Ok(1);
        }

        let mut lines: Vec<usize> = Vec::new();
        for sel in &selectors {
            let Some(task) = ctx.store.find(sel) else {
                eprintln!("TODO: no open task matches '{sel}'");
                return Ok(1);
            };
            lines.push(task.line_no);
        }
        lines.sort_unstable();
        lines.dedup();

        if dry {
            let nums: Vec<String> = lines.iter().map(|n| n.to_string()).collect();
            println!("TODO: would mark done: {}", nums.join(" "));
            return Ok(0);
        }

        let today = date::today();
        let mut archived: Vec<Task> = Vec::new();
        for &line_no in lines.iter().rev() {
            if let Some(task) = ctx.store.task_by_line_mut(line_no) {
                task.mark_done(today.clone());
                if ctx.config.auto_archive {
                    archived.push(task.clone());
                    ctx.store.remove_line(line_no);
                }
            }
        }
        if !archived.is_empty() {
            archived.reverse();
            let rendered: Vec<String> = archived.iter().map(Task::render).collect();
            append(&ctx.config.done_file, &rendered)?;
        }
        ctx.store.save()?;
        println!("TODO: {} marked done.", lines.len());
        Ok(0)
    });
}

fn append(path: &std::path::Path, lines: &[String]) -> todo_core::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    Ok(())
}
