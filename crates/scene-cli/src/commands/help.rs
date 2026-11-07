//! `help [topic]`: one section of the spec instead of the whole document.

use std::process::ExitCode;

use scene_server::help::{self, Help};

use crate::common::usage;

pub(crate) fn help(args: Vec<String>) -> ExitCode {
    let mut args = args.into_iter();
    let Some(name) = args.next() else {
        print!("{}", help::index());
        return ExitCode::SUCCESS;
    };
    if let Some(extra) = args.next() {
        return usage(format!("unexpected argument `{extra}`"));
    }
    match help::topic(&name) {
        Ok(Help::Text(text)) => print!("{}", text.trim_end().to_owned() + "\n"),
        Ok(Help::Commands) => crate::print_help(),
        Err(message) => return usage(message),
    }
    ExitCode::SUCCESS
}
