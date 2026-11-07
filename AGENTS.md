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
- Keep it blazing fast: follow "Performance" below; never merge a measured
  slowdown without saying why in the commit message.
- Keep changes inside this repository. The older Python project at
  `~/Документы/PixelArt/Изометрия` is a reference/oracle and must not be edited.

## Build and verify

```bash
make fmt
make ci
```

Before claiming success, run `cargo test --workspace` and `cargo clippy
--workspace --all-targets -- -D warnings`. For changes that can affect speed,
also run `make bench` (see "Performance").

## Working on scenes and renders

Pixels are the output, so check them, not only the exit code:

1. **Build** the scene: `iso-scene render scene.scene -o out.png` (or
   `tiled`, `sprites`, `voxel`).
2. **Look** at the image before changing anything else. For Tiled maps,
   rasterize with `tmxrasterizer`; compare with `preview.png`.
3. **Fix** one thing, then build and look again.

Use `iso-scene help <topic>` for one section of the spec and
`iso-scene limits` for every limit instead of reading all of `docs/spec.md`.

## Performance

`iso-scene` must be blazing fast: previews in the VS Code extension rebuild on
every keystroke, and sprite sheets, animations and maps are rebuilt in game
asset pipelines. Speed is a feature with the same weight as correctness.

- **Measure, do not guess.** Benchmarks live in `crates/*/benches` (criterion).
  Before a change that can affect speed, save a baseline:
  `cargo bench -p <crate> -- --save-baseline before`; after it, compare with
  `cargo bench -p <crate> -- --baseline before` and quote the numbers in the
  commit message. A regression above ~5% needs a reason in the message.
- **Add a benchmark** for every new heavy path (a backend, an exporter, a
  geometry pass) before optimizing it.
- **Optimizations must not change output.** Pixels, SVG, VOX and JSON stay
  byte-identical. Prove it with a test (e.g. parallel drawing equals
  sequential drawing, an index finds exactly what a full scan finds) and by
  comparing the outputs of a corpus of examples before and after.
- **Parallelize independent work** with `rayon`: sheet cells, pages, animation
  frames, atlas sprites, image bands. Collect results in input order and
  report the first error in input order, so the thread count never changes
  the output or the diagnostics.
- **Do not wake the thread pool for tiny work.** Small images and single
  items run on the calling thread (see `MIN_PARALLEL_PIXELS` in
  `scene-render`).
- **Do work once.** Prepare geometry once and reuse it (e.g. animation frames
  share the still part of a scene); load files once; no allocation, system
  call or environment lookup inside per-pixel loops.
- **No quadratic geometry.** Spatial lookups go through an index (sorted
  arrays or a grid) that visits candidates in their original order.
- **Check limits before expensive work**, not after: prepare cheaply,
  validate sizes against `docs/spec.md` §9, then rasterize.

## Current state

- Specification: draft v1.
- Implementation: lexer, parser, evaluator, iso/ortho backends, TOML materials, voxel/Tiled export and walls are complete; LSP/MCP are next.
- Tests: lexer, parser, evaluator, iso/ortho projections and renders, examples, CLI. Generated PNGs are not committed.

## Adding a feature

1. Write or update the spec section and the grammar.
2. Add examples under `examples/` that exercise the feature.
3. Implement in the smallest crate that owns the behaviour.
4. Add unit, property and (where relevant) golden tests.
5. Update `CHANGELOG.md` once it exists.
