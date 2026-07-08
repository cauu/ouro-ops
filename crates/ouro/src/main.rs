use ouro::cli;

fn main() {
    let code = match cli::run(std::env::args().collect()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            error.exit_code()
        }
    };
    std::process::exit(code);
}
