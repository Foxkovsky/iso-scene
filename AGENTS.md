# AGENTS.md

Instructions for AI agents working in this repository.

## Mission

`iso-scene` is a compiler from a small C-like language (`.model` / `.scene`)
to pixel-art backends: 2:1 isometric, orthographic top/front/side, voxel and
Tiled. It is a public, permissively licensed Rust project.

## Read first

1. `docs/spec.md` — semantics and limits.
2. `docs/grammar.ebnf` — normative grammar.
3. `docs/ARCHITECTURE.md` — crates and pipeline.

Do not invent syntax or semantics that is not in the spec. If a change is
needed, update the spec and grammar first, then the code.

## Hard rules

- `unsafe` is forbidden.
- Never panic on user input. Return `scene_syntax::Diagnostic` with a span.
- No `unwrap`/`expect` on parsed data, no unchecked indexing in the parser or
  evaluator.
- Preserve determinism: stable ordering, no hash-map iteration in output paths.
- Enforce the limits from `docs/spec.md` §9 before allocating.
- Sandbox `include` and `material` paths to the project root.
- Backends consume `SceneGraph`; they must not parse source.
- Keep changes inside this repository. The older Python project at
  `~/Документы/PixelArt/Изометрия` is a reference/oracle and must not be edited.

## Build and verify

```bash
make fmt
make ci
```

Before claiming success, run `cargo test --workspace` and `cargo clippy
--workspace --all-targets -- -D warnings`.

## Current state

- Specification: draft v1.
- Implementation: lexer and parser are complete; evaluator and backends are next.
- Tests: lexer, parser (31 unit tests), examples-parsing integration test, CLI.

## Adding a feature

1. Write or update the spec section and the grammar.
2. Add examples under `examples/` that exercise the feature.
3. Implement in the smallest crate that owns the behaviour.
4. Add unit, property and (where relevant) golden tests.
5. Update `CHANGELOG.md` once it exists.
