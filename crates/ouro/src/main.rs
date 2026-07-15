use ouro::cli;
use ouro::output::ToolOutput;

fn main() {
    let code = match cli::run(std::env::args().collect()) {
        Ok(()) => 0,
        Err(error) => {
            let code = error.exit_code();
            if !error.is_reported() {
                let output =
                    ToolOutput::failure("ouro", format!("exit_{code}"), error.to_string());
                // TTY-aware like every other emit (p5-2): human on a terminal, JSON when captured.
                if ouro::output::print_json(&output).is_err() {
                    eprintln!("{error}");
                }
            }
            code
        }
    };
    std::process::exit(code);
}
