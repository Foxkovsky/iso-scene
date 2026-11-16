//! Deterministic robustness corpus: the parser and evaluator must never panic
//! on arbitrary text, only return a diagnostic.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use scene_core::{compile_model, compile_scene_with_fuel};
use scene_syntax::parse;

/// xorshift64*: a fixed seed keeps CI runs reproducible.
fn next(seed: &mut u64) -> u64 {
    let mut value = *seed;
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *seed = value;
    value
}

const TOKENS: &[&str] = &[
    "version",
    "1",
    ";",
    "render",
    "iso",
    "scale",
    "8",
    "object",
    "a",
    "(",
    "2",
    ",",
    ")",
    "{",
    "}",
    "box",
    "at",
    "size",
    "place",
    "for",
    "i",
    "in",
    "..",
    "if",
    "else",
    "let",
    "var",
    "=",
    "+",
    "-",
    "*",
    "/",
    "%",
    "true",
    "false",
    "&&",
    "||",
    "material",
    "include",
    "\"x.model\"",
    "decal",
    "facing",
    "+y",
    "glow",
    "animation",
    "frames",
    "duration",
    "once",
    "turn",
    "anchor",
    "center",
    "mirror",
    "x",
    "y",
    "merge",
    "use",
    "rot",
    "#112233",
    "1.5",
];

fn on_large_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(test)
        .expect("spawn")
        .join()
        .expect("robustness thread");
}

#[test]
fn random_token_soup_never_panics() {
    on_large_stack(token_soup);
}

fn token_soup() {
    let mut seed = 0x9e37_79b9_7f4a_7c15;
    for case in 0..1_000 {
        let count = usize::try_from(next(&mut seed) % 100).unwrap_or(0);
        let mut source = String::new();
        for _ in 0..count {
            source.push_str(TOKENS[usize::try_from(next(&mut seed) % 59).unwrap_or(0)]);
            source.push(' ');
        }
        let result = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(program) = parse(&source) {
                let here = Path::new(".");
                let _ = compile_scene_with_fuel(&program, here, here, 20_000);
                let _ = compile_model(&program, here);
            }
        }));
        assert!(result.is_ok(), "case {case} panicked on {source:?}");
    }
}

#[test]
fn random_unicode_never_panics() {
    on_large_stack(random_unicode);
}

fn random_unicode() {
    let mut seed = 0x2545_f491_4f6c_dd1d;
    for case in 0..500 {
        let length = usize::try_from(next(&mut seed) % 160).unwrap_or(0);
        let source: String = (0..length)
            .map(|_| {
                let value = (next(&mut seed) & 0x0010_ffff) as u32;
                char::from_u32(value).unwrap_or('x')
            })
            .collect();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = parse(&source);
        }));
        assert!(result.is_ok(), "case {case} panicked on {source:?}");
    }
}
