use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_help();
        return ExitCode::SUCCESS;
    };

    match command.as_str() {
        "-V" | "--version" => {
            println!("iso-scene {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "-h" | "--help" => {
            print_help();
            ExitCode::SUCCESS
        }
        "spec-version" => {
            println!("{}", scene_syntax::SPEC_VERSION);
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("iso-scene: unknown command `{other}`");
            eprintln!("Try `iso-scene --help`.");
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!("iso-scene {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage: iso-scene <command> [options]");
    println!();
    println!("Commands:");
    println!("  spec-version    print the implemented language version");
    println!();
    println!("Options:");
    println!("  -h, --help      show this help");
    println!("  -V, --version   show the program version");
    println!();
    println!("The parser and render backends are not implemented yet.");
}

#[cfg(test)]
mod tests {
    #[test]
    fn spec_version_is_exposed() {
        assert_eq!(scene_syntax::SPEC_VERSION, 1);
    }
}
