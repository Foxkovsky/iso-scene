# Architecture

## Pipeline

```
source (.model / .scene)
   │
   ├─ scene-syntax   lexer → parser → AST + diagnostics
   │
   ├─ scene-core     name/type validation → evaluation → SceneGraph
   │
   └─ backend        iso | top | front | side | voxel | tiled
                        │
                        └─ PNG / SVG / VOX / TMJ
```

`SceneGraph` is the only interface between the language and rendering. It
contains:

- axis-aligned solids (boxes, cylinders, spheres, slants, ramps) in blocks;
- transformed instances with world-space vertices;
- material references;
- light and background settings;
- bounds.

Backends never parse source and never know about `.model` or `.scene`.

## Crates

| Crate | Responsibility |
|---|---|
| `scene-syntax` | tokens, spans, parser, AST, diagnostics |
| `scene-core` | validation, evaluation, fuel, `SceneGraph` |
| `scene-render` | shared rasterizer, image, palettes, primitive meshes |
| `scene-backend-iso` | 2:1 isometric rasterizer |
| `scene-backend-ortho` | top / front / side rasterizer |
| `scene-backend-voxel` | occupancy sampling → VOX |
| `scene-export-tiled` | TMJ + tileset JSON |
| `scene-cli` | build, preview, export |
| `scene-server` | JSON-lines protocol for the VS Code extension |

## Model and scene

- `.model` — reusable object templates. No camera, no scale, no placement.
- `.scene` — includes models, places them, selects light and projection.
- The same grammar serves both; the file extension narrows the allowed
  top-level items.

This separation is what makes one object usable in isometric, top-down and
voxel outputs.

## Determinism

Same input and options must produce byte-identical output.

Rules:

- stable iteration order everywhere; no `HashMap` iteration in rendering;
- total order for face sorting (depth, then z, then a stable tie-breaker);
- no wall-clock, randomness or environment-dependent behavior in v1;
- procedural textures use an explicit seed;
- floating-point math is `f64`; rounding to pixels happens at one place per
  backend.
- parallel work never changes output: rasterization runs over horizontal
  bands that each replay the full shape list in order, and independent items
  (sheet cells, pages, frames, atlas sprites) are collected in input order
  with the first error in input order winning.

## Performance

Rendering is parallel and incremental where it can be without changing a
pixel:

- `scene_render::Framebuffer::draw` rasterizes a shape list over horizontal
  bands with `rayon`; each triangle row scans only its covered span, and
  lines skip the steps outside a band;
- merge groups find edge-splitting vertices through a uniform grid instead of
  a full scan;
- `scene_backend_iso::prepare_frames` builds animation frames as shape lists,
  checks their size, draws the still part of the scene once per frame size
  and redraws only the region the turning instance can touch;
- the sprite and Tiled exporters render cells, pages and atlas sprites in
  parallel.

`make bench` runs the criterion benchmarks of the iso and ortho backends and
of the sprite exporter. `AGENTS.md` lists the rules for performance changes.

## Bounded evaluation

The evaluator owns a fuel counter. Every statement and every loop iteration
spends one unit. Exhaustion produces `E0401` with a span. Parsing has separate
limits (file size, token count, nesting).

The invariant is simple and testable: **fuel strictly decreases**. Therefore
evaluation terminates for every input, including adversarial ones.

The parser and evaluator are recursive, so their depth is bounded in the
parser, before any recursion can run away: 128 nested blocks, parentheses and
unary operators, and expressions 256 levels deep, where every operator in a
chain like `a + b + c` is a level. At these limits a release build needs under
512 KiB of stack (a Windows main thread has 1 MiB, Linux and macOS 8 MiB); a
debug build needs up to 4 MiB. `programs_at_the_nesting_limits_fit_a_small_stack`
keeps that true.

## Safety

- `#![forbid(unsafe_code)]` in every crate.
- The language performs no network or process I/O.
- Includes and material paths are canonicalized and confined to the project
  root. Symlinks and `..` escapes are rejected.
- All allocations derived from input are capped (see `docs/spec.md`, §9).
- Render dimensions are capped before allocating buffers.

## Testing strategy

| Layer | Tool | Property |
|---|---|---|
| unit | `cargo test` | lexer/parser/evaluator cases |
| property | `proptest` | parse/format round-trip, mirror involution, rotation composition, bbox containment |
| fuzz | `cargo-fuzz` | no panic, no hang, no OOM on arbitrary bytes and ASTs |
| golden | `insta` + PNG | pixel output does not regress |
| differential | Python reference | Rust matches the reference implementation during the port |
| bounded verification | `kani` | parser bounds, fuel decrease, rotation normalization |
| mutation | `cargo-mutants` | tests actually detect changes |

Fuzz targets:

1. `parse` — arbitrary bytes;
2. `eval` — arbitrary AST, including loops with random bounds;
3. `render_iso` / `render_ortho` / `voxel` — bounded scenes;
4. `include` — path traversal and cycles.

## Backends and style

Projection is mathematics; pixel-art style is a backend option. Later versions
may expose per-backend settings such as wall height exaggeration, outline
rules, palette mapping and dithering. The model remains unchanged.

## Versioning

`version 1` is the language contract. Breaking changes bump the version and
ship with a converter. Golden tests are grouped by language version so old
scenes keep rendering.
