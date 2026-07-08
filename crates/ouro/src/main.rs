use ouro::cli;
use ouro::output::ToolOutput;

fn main() {
    let code = match cli::run(std::env::args().collect()) {
        Ok(()) => 0,
        Err(error) => {
            let code = error.exit_code();
            let output = ToolOutput::failure("ouro", format!("exit_{code}"), error.to_string());
            match serde_json::to_string(&output) {
                Ok(line) => println!("{line}"),
                Err(_) => eprintln!("{error}"),
            }
            code
        }
    };
    std::process::exit(code);
}
