# iso-scene

[![CI](https://github.com/Foxkovsky/iso-scene/actions/workflows/ci.yml/badge.svg)](https://github.com/Foxkovsky/iso-scene/actions/workflows/ci.yml)
![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange)
![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

**A compiler from a small C-like language to pixel-art game assets.**

Describe an object once with simple solids. iso-scene renders it as 2:1
isometric pixel art, orthographic top/front/side views, SVG, a MagicaVoxel
model, sprite sheets with animations, or a ready-to-edit Tiled level.

```
.model ─┐                       ┌─▶ PNG / SVG        iso 2:1, top, front, side
        ├─▶ parse ─▶ evaluate ─▶├─▶ VOX              MagicaVoxel, chunked
.scene ─┘   (bounded, fuel)     ├─▶ sprite sheets    JSON + Tiled tileset + Phaser/PixiJS atlas
.walls ─────────────────────────┴─▶ Tiled map        floor, walls, shadows, objects, collisions
```

> **Status: pre-1.0.** The language is specified (draft v1) and fully
> implemented; syntax may still change before 1.0, with a converter for any
> breaking change.

## Contents

- [Features](#features)
- [Install](#install)
- [A first scene](#a-first-scene)
- [The language at a glance](#the-language-at-a-glance)
- [Outputs](#outputs)
- [Editors and agents](#editors-and-agents)
- [Guarantees](#guarantees)
- [Documentation](#documentation)
- [Development](#development)
- [Repository layout](#repository-layout)
- [Roadmap](#roadmap)
- [License](#license)

## Features

- **One model, every view.** Iso, top, front and side renders from the same
  geometry; per-pixel depth, sun and ambient light, TOML materials with
  procedural textures.
- **Game-ready exports.** Sprite sheets (1–16 directions, animation rows,
  pivots, padding, multi-page), a `sheet.json` with a JSON Schema, Tiled
  tilesets, Phaser/PixiJS atlases, MagicaVoxel models.
- **Tiled levels.** Floor, 16-way autotiled walls with a Wang set, contact
  shadows, depth-sorted objects that land exactly where Tiled draws them,
  object footprints and collision warnings, and a Tiled extension that checks
  collisions as you edit.
- **Animation in the language.** `animation open (t) frames 7 duration 100;`
  and any expression of `t`: doors swing (`turn 90 * t`), drawers slide.
- **Fast.** Parallel, band-split rasterizer; incremental animation frames;
  criterion benchmarks guard every hot path. A classroom of 40 objects
  renders in about 25 ms.
- **Safe on any input.** No `unsafe`, no panics on user input, every limit
  checked before allocation, evaluation bounded by fuel, includes sandboxed
  to the project.
- **Editor and agent support.** A language server, an MCP server that hands
  agents the rendered image, and a VS Code extension with live preview.

## Install

```bash
git clone https://github.com/Foxkovsky/iso-scene
cd iso-scene
cargo install --path crates/scene-cli     # installs the `iso-scene` binary
```

Requires Rust 1.85 or newer. No Python, Node or GPU is needed.

## A first scene

`crate.scene`:

```c
version 1;

object crate (1, 1) {
    box at (0, 0, 0) size (w, d, 1) material wood;
}

render iso;
scale 32;

place crate at (0, 0, 0);
place crate at (1.2, 0, 0) rot 90;
```

```bash
iso-scene render crate.scene -o crate.png          # PNG
iso-scene render crate.scene -o crate.svg          # SVG
iso-scene render crate.scene --view top -o top.png # orthographic view
```

Then look at the image, change one thing, and render again.

## The language at a glance

Models (`.model`) define reusable objects; scenes (`.scene`) include models,
place them and choose the projection. Every length is in **blocks**, the cell
of the map; `scale` (pixels per block) belongs to the scene, never to the
model.

```c
// desk.model
version 1;
material oak = "materials/oak.toml";
material steel = "materials/steel.toml";

object desk (5, 2) {                       // footprint w × d in blocks
    let leg = 0.16;
    box at (0.16, 0.16, 0) size (leg, leg, 2.8) material steel;
    mirror x;                              // four legs from one
    mirror y;
    box at (0, 0, 2.8) size (w, d, 0.16) material oak;
}
```

```c
// door.scene
version 1;
include "desk.model";

object door (2, 0.2) {
    box at (0, 0, 0) size (w, d, 4) material oak;
}

render iso;
scale 24;

animation open (t) frames 7 duration 100 once;

for i in 0..3 {
    place desk at (i * 6, 0, 0) rot 0 anchor back;
}
place door at (-3, 2, 0) turn 80 * t;      // hinge at the door's local (0, 0)
```

Solids: `box`, `cyl`, `sphere`, `slantbox`, `slantcyl`, `ramp`. Also `let` /
`var`, `for`, `if`, `mirror`, reusable parts with `use`, and `merge` for
seamless welded surfaces. `iso-scene help <topic>` prints the relevant
section of the spec.

## Outputs

| Command | Produces |
|---|---|
| `render scene -o out.png` / `out.svg` | iso or orthographic PNG, iso SVG |
| `voxel scene -o out.vox` | MagicaVoxel model, split into chunks past 256 per axis |
| `sprites a.model b.scene --views 8 --atlas` | sprite sheet, `sheet.json`, Tiled tilesets, Phaser/PixiJS atlas |
| `animate scene` | each `animation` as a strip, an animated Tiled tile and a map |
| `tiled scene --walls room.walls` | isometric Tiled map with floor, walls, shadows, objects, collisions |
| `check` / `eval` | diagnostics / the evaluated scene graph |
| `help [topic]` / `limits` | one section of the spec / every limit with its error code |

Diagnostics look like ``room.scene:4:1: error[E0301]: unknown object `sofa` ``;
codes are listed in the spec.

## Editors and agents

**VS Code.** `editors/vscode`: live preview (iso, drawing, voxels), errors in
Problems as you type, hover, go to definition, outline, completion, build and
sprite-sheet commands.

```bash
cd editors/vscode && npx @vscode/vsce package
code --install-extension iso-scene-0.4.0.vsix
```

**Any LSP editor.** `iso-scene lsp` speaks the Language Server Protocol over
stdio (Neovim and Helix setups in [`docs/servers.md`](docs/servers.md)).

**AI agents (MCP).** `iso-scene mcp` exposes `check`, `render` (returns the
image), `export`, `help` and `limits`. For Claude Code:

```bash
claude mcp add iso-scene -- iso-scene mcp
```

**Tiled.** `editors/tiled/iso-scene.js` checks collisions on the current
object positions (Map → Check iso-scene Collisions).

## Guarantees

- **Deterministic output.** The same input gives byte-identical PNG, SVG, VOX
  and JSON on every machine and thread count.
- **Diagnostics, not crashes.** Malformed or hostile input (deep nesting,
  huge loops, oversized grids) ends in a diagnostic with a span; the limits
  are listed by `iso-scene limits` and tested against the spec.
- **No surprises on disk.** `include` and `material` paths stay inside the
  project root; so do all paths of the MCP server.

## Documentation

| Document | Covers |
|---|---|
| [`docs/spec.md`](docs/spec.md) | language semantics, limits, diagnostics (normative) |
| [`docs/grammar.ebnf`](docs/grammar.ebnf) | grammar (normative) |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | pipeline, crates, determinism, performance |
| [`docs/backends.md`](docs/backends.md) | renderers, Tiled export, sprite sheets, animation |
| [`docs/walls.md`](docs/walls.md) | `.walls` format, wall layers and shadows in Tiled |
| [`docs/projections.md`](docs/projections.md) | projection formulas and their tests |
| [`docs/servers.md`](docs/servers.md) | language server and MCP server |
| [`docs/sprite-sheet.schema.json`](docs/sprite-sheet.schema.json) | the `sheet.json` schema |

Examples live in [`examples/`](examples): a chair, a desk, a classroom in iso
and top view, and an animated door.

## Development

```bash
make fmt          # format
make ci           # fmt-check, clippy -D warnings, tests, docs
make bench        # criterion benchmarks
make vscode-test  # VS Code extension tests in Node
make vscode-it    # the extension inside VS Code (needs code, xvfb-run)
```

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md). Any
change to syntax or semantics updates `docs/spec.md` and `docs/grammar.ebnf`
in the same commit; a change that can affect speed quotes benchmark numbers.

## Repository layout

```
crates/scene-syntax/          lexer, parser, AST, diagnostics
crates/scene-core/            evaluation, scene graph, limits
crates/scene-render/          rasterizer, images, materials
crates/scene-backend-iso/     2:1 isometric renderer and SVG
crates/scene-backend-ortho/   top / front / side renderer
crates/scene-backend-voxel/   voxelizer and VOX writer
crates/scene-walls/           .walls format, autotiling, shadows
crates/scene-export-sprites/  sprite sheets and animations
crates/scene-export-tiled/    Tiled maps, footprints, collisions
crates/scene-server/          help topics, limits, LSP and MCP servers
crates/scene-cli/             the `iso-scene` command
editors/vscode/               VS Code extension
editors/tiled/                Tiled extension
docs/                         specification and guides
examples/                     models and scenes
```

## Roadmap

| Done | Next |
|---|---|
| language, evaluator, iso/ortho/SVG/voxel backends | formatter |
| sprite sheets, animations in the language | browser navigator with on-device WASM rendering |
| Tiled export with walls, shadows and collisions | parametric objects (`object name (w, d)` overrides) |
| language server, MCP server, VS Code extension | frozen grammar and 1.0 |

## License

Code: **MIT OR Apache-2.0**. Assets and examples: **CC BY 4.0**.
See [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE),
[`LICENSE-ASSETS`](LICENSE-ASSETS).

Русская версия: [`README.ru.md`](README.ru.md).
