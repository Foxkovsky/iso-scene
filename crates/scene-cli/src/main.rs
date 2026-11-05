use std::fs;
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
        "check" => {
            let Some(path) = args.next() else {
                eprintln!("iso-scene: `check` needs a file");
                return ExitCode::from(2);
            };
            check(&path)
        }
        other => {
            eprintln!("iso-scene: unknown command `{other}`");
            eprintln!("Try `iso-scene --help`.");
            ExitCode::from(2)
        }
    }
}

fn check(path: &str) -> ExitCode {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("iso-scene: cannot read `{path}`: {error}");
            return ExitCode::from(2);
        }
    };
    match scene_syntax::parse(&source) {
        Ok(program) => {
            println!(
                "{path}: ok (version {}, items: {})",
                program.version,
                program.items.len()
            );
            ExitCode::SUCCESS
        }
        Err(diagnostic) => {
            let (line, column) = line_column(&source, diagnostic.span.start);
            eprintln!(
                "{path}:{line}:{column}: error[{}]: {}",
                diagnostic.code, diagnostic.message
            );
            ExitCode::from(1)
        }
    }
}

/// Convert a byte offset to a 1-based line and column.
fn line_column(source: &str, offset: u32) -> (usize, usize) {
    let offset = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(source.len());
    let prefix = source.get(..offset).unwrap_or(source);
    let mut line = 1;
    let mut column = 1;
    for character in prefix.chars() {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn print_help() {
    println!("iso-scene {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage: iso-scene <command> [options]");
    println!();
    println!("Commands:");
    println!("  check <file>    parse a .model or .scene file and report diagnostics");
    println!("  spec-version    print the implemented language version");
    println!();
    println!("Options:");
    println!("  -h, --help      show this help");
    println!("  -V, --version   show the program version");
    println!();
    println!("Evaluation and render backends are not implemented yet.");
}

#[cfg(test)]
mod tests {
    use super::line_column;

    #[test]
    fn spec_version_is_exposed() {
        assert_eq!(scene_syntax::SPEC_VERSION, 1);
    }

    #[test]
    fn offsets_map_to_lines_and_columns() {
        assert_eq!(line_column("", 0), (1, 1));
        assert_eq!(line_column("a\nbc", 0), (1, 1));
        assert_eq!(line_column("a\nbc", 1), (1, 2));
        assert_eq!(line_column("a\nbc", 2), (2, 1));
        assert_eq!(line_column("a\nbc", 3), (2, 2));
        assert_eq!(line_column("a\nbc", 99), (2, 3));
    }
}
