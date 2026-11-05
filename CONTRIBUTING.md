# Contributing

## Prerequisites

- Rust 1.85 or newer (`rust-toolchain.toml` pins stable);
- `cargo`, `rustfmt`, `clippy`;
- no Python, Node or GPU is required for the core crates.

## Build and test

```bash
make fmt        # format
make ci         # fmt-check + clippy + test + doc
make run ARGS="--version"
```

`cargo test --workspace` must stay green. CI also builds on macOS and Windows.

## The specification is the source of truth

The language is defined by:

- [`docs/spec.md`](docs/spec.md) — semantics;
- [`docs/grammar.ebnf`](docs/grammar.ebnf) — syntax;
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — pipeline.

Any change to syntax or behaviour must update the spec and the grammar **in the
same commit** as the code. A change that only edits the parser is incomplete.

## Rules for code

- `#![forbid(unsafe_code)]` everywhere. Do not add an exception without a
  written justification in the PR.
- No panics on user input: no `unwrap`, `expect`, indexing or slicing that can
  fail on parsed data. Return a `Diagnostic` with a span instead.
- No `HashMap` iteration in rendering or diagnostics ordering. Determinism is
  part of the contract.
- Keep backends independent: they consume `SceneGraph` and never parse source.
- Every limit in `docs/spec.md` §9 must be enforced before allocating.

## Tests

- Unit tests next to the code for lexer, parser, evaluator and each backend.
- Property tests (`proptest`) for round-trips and geometry invariants.
- Fuzz targets (`cargo-fuzz`) for `parse`, `eval` and each renderer. A crash is
  a release blocker.
- Golden tests (`insta`) for pixel output. Do not update a golden image without
  explaining the visual change in the commit message.
- If you fix a bug, add the smallest input that reproduces it.

## Commits

Short imperative subject with an area prefix:

```
spec: forbid while in v1
syntax: report spans for unterminated comments
iso: fix z-buffer tie at equal depth
```

Keep formatting-only changes separate from behaviour changes.

## License

By contributing you agree that code is licensed under MIT OR Apache-2.0 and
assets under CC BY 4.0.
