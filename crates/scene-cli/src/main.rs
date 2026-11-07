//! `iso-scene` command-line interface.

mod commands;
mod common;

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
            commands::inspect::check(&path)
        }
        "eval" => commands::inspect::eval(args.collect()),
        "render" => commands::render::render_scene(args.collect()),
        "voxel" => commands::voxel::voxel_scene(args.collect()),
        "tiled" => commands::tiled::tiled_scene(args.collect()),
        "animate" => commands::animate::animate_scene(args.collect()),
        "sprites" => commands::sprites::sprites_scene(args.collect()),
        "limits" => commands::limits::limits(args.collect()),
        "help" => commands::help::help(args.collect()),
        other => {
            eprintln!("iso-scene: unknown command `{other}`");
            eprintln!("Try `iso-scene --help`.");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn print_help() {
    println!("iso-scene {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage: iso-scene <command> [options]");
    println!();
    println!("Commands:");
    println!("  check <file>    parse a .model or .scene file and report diagnostics");
    println!("  eval <file>     parse and evaluate to a scene graph");
    println!("  render <file>   render a .scene to PNG (iso or ortho); -o out.svg writes iso SVG");
    println!("  voxel <file>    export a .scene as a MagicaVoxel .vox model");
    println!("  tiled <file>    export a .scene as an isometric Tiled map");
    println!("  animate <file>  render a rotation animation as a sprite sheet + animated tile");
    println!("                  --object <name> --from --to --frames --duration [--base <dir>]");
    println!(
        "  sprites <file>  pack objects from .scene/.model files into a sprite sheet + JSON/TSJ"
    );
    println!("                  --views 1|2|4|8|16 or --angles 0,45,90,...");
    println!("                  --object <name> keeps, --exclude <name> drops objects");
    println!("                  --pivot front|back|left|right|center anchors static views");
    println!("                  --shadow contact|none bakes or omits the contact shadow");
    println!("                  --padding <px> leaves transparent gaps around cells");
    println!("                  --debug-grid also writes <name>.debug.png with cell borders");
    println!("                  --atlas also writes <name>.atlas.json for Phaser/PixiJS");
    println!("                  --max-rows <n> splits the sheet into pages of n rows");
    println!("                  --projection iso|top|front|side (default: the scene's render)");
    println!(
        "                  --animation <name> --from --to --frames --duration adds a frame row"
    );
    println!(
        "                  (repeatable; --durations a,b,.. --once --label <name> per animation)"
    );
    println!("                  --walls <file> adds a 16-way wall tile layer with Wang set");
    println!("                  (near walls above the objects, the rest below them;");
    println!("                  --walls-behind puts all walls below, --walls-front above)");
    println!(
        "                  --tile-layer puts objects in a tile layer instead of an object layer"
    );
    println!("  help [topic]    one section of the language spec (topics without one)");
    println!("  limits          list every limit and its error code (--json for agents)");
    println!("  spec-version    print the implemented language version");
    println!();
    println!("Options:");
    println!("  -h, --help      show this help");
    println!("  -V, --version   show the program version");
    println!();
    println!("Evaluation options:");
    println!(
        "  --fuel <n>      evaluation budget for `eval` (default {})",
        scene_core::DEFAULT_FUEL
    );
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use crate::common::line_column;

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
