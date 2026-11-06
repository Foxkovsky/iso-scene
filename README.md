# iso-scene

**A 3D model and scene compiler for pixel-art graphics.**

Describe an object once with simple solids, then render it as 2:1 isometric
pixel art, an orthographic top or side view, a voxel model, or a Tiled level.

> Status: **pre-alpha**. The language specification is being frozen; the Rust
> implementation is next. Nothing is stable yet.

## Why

Hand-drawing every view of every object does not scale. A 3D model gives you
all views for free:

```
.model  ──▶  geometry + materials + loops
                  │
                  ├── iso 2:1     ──▶ PNG / SVG
                  ├── top / side  ──▶ PNG / SVG
                  ├── voxel       ──▶ VOX
                  └── Tiled       ──▶ TMJ + tilesets
.scene  ──▶  placements, light, scale, exports
```

## Design principles

- **One unit: the block.** A block is the grid cell of the map. Physical size
  (10 cm, 25 cm, 1 m) is a project convention, not a language feature.
- **Resolution-independent models.** `scale` (pixels per block) belongs to the
  scene or render command, never to the model.
- **C-like syntax.** `;`, `{}`, `//` and `/* */`; whitespace and newlines are
  not significant.
- **Bounded evaluation.** Loops are allowed, but every evaluation step spends
  fuel. A malformed scene fails with a diagnostic instead of hanging the
  editor.
- **Models and scenes are separate.** `.model` files define reusable objects;
  `.scene` files place them, set light and projection, and export.

## Quick start

```bash
cargo run -p scene-cli -- --version
cargo run -p scene-cli -- check examples/chair.model
cargo run -p scene-cli -- eval examples/chair.model
cargo run -p scene-cli -- eval examples/classroom.scene
cargo run -p scene-cli -- render examples/classroom.scene -o out/classroom.png
cargo run -p scene-cli -- render examples/classroom_top.scene -o out/classroom_top.png
cargo run -p scene-cli -- voxel examples/classroom.scene --density 7 -o out/classroom.vox
```

The lexer, parser, evaluator, iso/ortho backends, TOML materials and voxel
export are implemented. Tiled export is next. The normative documents are:

- [`docs/spec.md`](docs/spec.md) — language semantics;
- [`docs/grammar.ebnf`](docs/grammar.ebnf) — grammar;
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — pipeline and crates;
- [`docs/projections.md`](docs/projections.md) — projection formulas and their tests.

Examples: [`examples/chair.model`](examples/chair.model),
[`examples/classroom.scene`](examples/classroom.scene).

Render outputs and other generated binaries are gitignored; rebuild them from
the sources with the commands above.

## Repository layout

```
crates/scene-syntax/        lexer, parser, AST, diagnostics
crates/scene-core/          validation, evaluation, scene graph
crates/scene-render/       shared rasterizer, image, materials, mesh
crates/scene-backend-iso/   2:1 isometric renderer
crates/scene-backend-ortho/ top / front / side renderer
crates/scene-backend-voxel/ voxelizer
crates/scene-export-tiled/  Tiled JSON export
crates/scene-cli/           command line
crates/scene-server/        JSON-lines server for the VS Code extension
docs/                       specification and architecture
examples/                   models and scenes
```

## Roadmap

| Milestone | Scope |
|---|---|
| v0.1 | parser, validator, evaluator, iso backend, CLI |
| v0.2 | scenes, top/ortho backends, TOML materials, voxel export |
| v0.3 | Tiled export, sprite sheets, backend style options |
| v1.0 | frozen grammar, formatter, LSP, public release |

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md). Changes to syntax or semantics must
update `docs/spec.md` and `docs/grammar.ebnf` together.

## License

Code: **MIT OR Apache-2.0**. Assets and examples: **CC BY 4.0**.
See `LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE-ASSETS`.

Русская версия: [`README.ru.md`](README.ru.md).
