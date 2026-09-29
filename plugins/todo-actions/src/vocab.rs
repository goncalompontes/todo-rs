use todo_core::vocab::Vocab;

const USAGE: &str = "  vocab [add AXIS VALUE...]\n    Show or extend the allowed vocab values.";

pub fn run() {
    todo_plugin::main("vocab", USAGE, |ctx| {
        let Some(path) = ctx.config.vocab_file.clone() else {
            eprintln!("TODO: no vocab file configured (TODO_VOCAB_FILE)");
            return Ok(1);
        };
        let mut vocab = Vocab::load(&path).unwrap_or_default();

        if ctx.args.is_empty() {
            print!("{}", ctx.colors.dim());
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| vocab.render());
            print!("{}", text.trim_end());
            println!("{}", ctx.colors.rst());
            return Ok(0);
        }

        match ctx.args[0].as_str() {
            "add" => {
                if ctx.args.len() < 3 {
                    eprintln!("TODO: usage: todo vocab add AXIS VALUE...");
                    return Ok(1);
                }
                let axis = &ctx.args[1];
                let values = &ctx.args[2..];
                vocab.add(axis, values);
                std::fs::write(&path, vocab.render())?;
                println!("TODO: added to {axis}: {}", values.join(" "));
                Ok(0)
            }
            axis => match vocab.values(axis) {
                Some(values) => {
                    println!("{axis}: {}", values.join(" "));
                    Ok(0)
                }
                None => {
                    eprintln!("TODO: unknown vocab axis '{axis}'");
                    Ok(1)
                }
            },
        }
    });
}
