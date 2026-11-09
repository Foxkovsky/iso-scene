//! `limits`: every limit from `docs/spec.md` §9, read from the code.

use std::process::ExitCode;

use crate::common::{Args, Parsed, usage};

pub(crate) fn limits(args: Vec<String>) -> ExitCode {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn run(args: Vec<String>) -> Parsed<ExitCode> {
    let mut json = false;
    for argument in Args::new(args) {
        match argument.as_str() {
            "--json" => json = true,
            _ => return Err(usage(format!("unexpected argument `{argument}`"))),
        }
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&scene_server::limits::json()).unwrap_or_default()
        );
    } else {
        print!("{}", scene_server::limits::text());
    }
    Ok(ExitCode::SUCCESS)
}
