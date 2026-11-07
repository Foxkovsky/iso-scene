# Backends

This document fixes the parts of rendering that the language contract depends
on. It will grow with the implementation.

## Built-in materials

Built-in names are always available. A scene or model may declare a TOML
material alias; the renderer loads the file once per render. Flat tones are
derived from `color`: `top = color`, `right = 0.78 * color`,
`left = 0.59 * color`; `outline` is used as-is. `roughness`, `metallic` and
`specular` are validated but not used by flat shading yet, and texture blocks
are accepted but ignored.

Used when a shape omits `material`. Values are RGB, one tone per visible face
plus an outline. A project should normally declare `.toml` materials instead.

| Name | top | right | left | outline |
|---|---|---|---|---|
| `wood` | `#B0B2B6` | `#8A8D92` | `#686B70` | `#26282E` |
| `metal` | `#969AA0` | `#75797F` | `#585C62` | `#22242A` |
| `light` | `#CED0D4` | `#A8ABB0` | `#84878D` | `#2C2E34` |
| `glass` | `#96BEC8` | `#789EA8` | `#5E808A` | `#28343A` |
| `dark` | `#6E7076` | `#56585E` | `#404248` | `#1E2026` |
| `red` | `#A4181A` | `#841216` | `#6C1014` | `#28191B` |
| `white` | `#F4F4F0` | `#E6E7E4` | `#D7D9D7` | `#26282E` |

## iso

```
screen_x = (x - y) * scale
screen_y = (x + y) * scale / 2 - z * scale
```

- One block on the floor is a `2*scale × scale` diamond.
- Vertical size is not foreshortened.
- At `scale = 32` the tile grid is 64×32.
- Faces are sorted back-to-front with a z-buffer; ties use a stable order.

## top

Orthographic from +Z:

```
screen_x = x * scale
screen_y = y * scale
depth    = z
```

One block is `scale × scale` pixels.

## front

Orthographic from -Y:

```
screen_x = x * scale
screen_y = -z * scale
depth    = y
```

## side

Orthographic from +X:

```
screen_x = y * scale
screen_y = -z * scale
depth    = x
```

## Output

- No anti-aliasing. Pixels are filled by the same coverage rule for every
  backend; the exact rule is fixed together with the first golden test.
- The image bounds are the integer bounding box of all projected vertices,
  expanded to at least `canvas`.
- Background is transparent unless `background` is set.
- Output must be byte-identical for identical input and options.

## voxel

Occupancy sampling of the same `SceneGraph`:

- `step = 1 / density` blocks per voxel;
- grid size `ceil(extent / step)`; axes over 256 are split into several
  models joined by a `nTRN`/`nGRP`/`nSHP` scene graph, total at most 256³
  voxels and 4096 per axis;
- material `color` is used as the voxel palette colour;
- output is a `MagicaVoxel` VOX file (SIZE, XYZI, RGBA);
- overlaps are resolved in instance/part order: later parts win.

## Tiled

`iso-scene tiled` writes:

- `scene.tmj` — isometric map, layers `floor`, `objects` and a hidden
  `collision` layer (plus the wall layers with `--walls`, see "Walls");
- `floor.tsj` + `floor_tile.png` — one diamond floor tile;
- `objects.tsj` + `objects.png` — atlas of unique object/rotation sprites;
- `walls.tsj`/`walls.png` and `wall_shadow.tsj`/`wall_shadow.png` with
  `--walls`;
- `preview.png` — the normal iso render.

If the map looks semi-transparent when opened in Tiled, that is the editor,
not the export: **View → Highlight Current Layer** draws layers above the
selected one at 40% opacity and darkens the layers below. The exported map
always has `opacity = 1` on every layer and only binary alpha in the PNGs.

Objects are written as tile objects in an `objectgroup` layer, ordered by
depth (`x + width + y + depth + z`, far to near). This allows several objects
in one cell and objects above the floor. The tileset has no `tileoffset`; each
object is positioned by its sprite anchor and the map's isometric origin.

Tiled stores isometric object positions **unprojected**: both axes count
`tileheight` pixels per cell, and Tiled projects them itself
(`IsometricRenderer::pixelToScreenCoords`). An object's position is the
footprint corner nearest the viewer (its sprite's bottom-center), times
`tileheight`; an elevation of `z` blocks is `z` cells back along both axes.
A 0.001 px nudge into the cell keeps Tiled's own cell lookup on that cell
instead of rounding to the next one at the shared corner. The placement is
checked with `tmxrasterizer` against the iso render of the same scene.

### Footprints and collisions

Every object carries its declared footprint (the `w × d` of `object`, turned
by `rot`): `footprint_x/y` (cells, relative to the object's own position, so
the footprint follows the object when it is moved in Tiled), `footprint_w/h`,
and the height range `bottom`/`top`. The hidden `collision` layer has the same
footprints as rectangles, which Tiled draws as the diamonds the objects stand
on; switch it on to see what each object occupies.

The exporter warns (on stderr; the map is still written) about objects whose
footprints and height ranges overlap, and about objects on wall cells. Objects
side by side or stacked are fine. The check buckets objects by cell, and the
footprints may cover at most 4 000 000 cells in total (`E0602`).

`editors/tiled/iso-scene.js` is a Tiled extension with the same check:
**Map → Check iso-scene Collisions** selects the colliding objects, lists them
in the Console and redraws the `collision` layer from the current positions.
`tiled --evaluate editors/tiled/iso-scene.js map.tmj` runs it without the
editor.

### Objects as tiles

`--tile-layer` writes the sprites into a tile layer named `objects` instead of
an object layer: one tile per cell, in depth order. Tiled draws a tile image
with its bottom-left at the cell's position, so the object tileset gets a
`tileoffset` of `(tileWidth - atlasWidth) / 2`. With that offset the sprite's
bottom-center lands on the cell's bottom vertex, and a tile placed by hand in
Tiled covers the cell it stands on. A tile layer holds one sprite per cell,
so objects sharing a cell (a chair under a table, a box on a shelf) go to
extra layers `objects-2`, `objects-3`, …, in depth order: the n-th object in
a cell is on the n-th layer. At most 16 objects may share a cell in this mode
(`E0602`); the object layer has no such limit. A tile layer has no
elevation, so a raised object is drawn on the cell under it.

The `floor` layer remains a regular tile layer. Because object order is stored
in the object layer, the Tiled `draworder` is `index`; do not sort objects in
the Tiled UI if you want to preserve the generated depth order.

## Sprite sheets

`iso-scene sprites` packs object renders into a uniform grid, without a map.
It is the format a game engine consumes directly:

```bash
iso-scene sprites room.scene --views 4 -o out/sprites
iso-scene sprites chair.model desk.model --views 4 --scale 8 -o out/furniture
iso-scene sprites door.scene --views 4 \
    --animation door_leaf --from 0 --to 90 --frames 7 --duration 100 \
    -o out/door
```

Input is one or more `.scene` or `.model` files. A scene contributes the
objects it actually places (each object once, in placement order); a model
contributes all objects defined in it, including helpers such as a
`chair_leg` used by `chair`. Use `--object <name>` (repeatable) to keep only
some of them, or `--exclude <name>` (repeatable) to drop some. The pixel scale is taken from the first scene; models
and mixed inputs use `--scale` (default 16).

Objects are identified by name. The same object reached from several files
(for example a model included by two scenes) is packed once. Two *different*
objects with one name are an error (exit code 2) instead of one silently
replacing the other; rename one or pick with `--object`. Likewise a material
name bound to two different files is an error.

`--name <stem>` sets the file names (`<stem>.png`, `.json`, `.tsj`). It must be
a bare stem: no `/`, `\`, `..` and no extension.

`--base <dir>` sets the directory relative paths are resolved from. When the
working directory does not contain it, `<dir>` also becomes the project root
that `include` and `material` paths must stay inside, so a sheet can be built
from any directory. This applies to `render`, `voxel` and `tiled` too.

A sheet holds at most 4 096 cells (objects × views + frames) and 1 024
animation frames; these limits are checked before anything is rendered.

`--views 1|2|4|8|16` selects the static sides: `0°`, `0°/180°` (front and
back), the four quarter turns, or 8/16 evenly spaced directions (45° and
22.5° steps). `--angles` picks any list of yaws in degrees, e.g.
`--angles 0,90` for front and side. Quarter turns are exact; other angles turn
the object freely, and the pivot is computed with the same transform, so every
direction stands on the same floor line. `rotation` in the manifest is the
view's yaw (an integer for whole degrees).

Views always show the object itself in canonical orientation; the `rot` of a
`place` in a scene does not change the sheet (a scene contributes *which*
objects are packed, not how they are turned).

`--projection iso|top|front|side` picks the camera. It defaults to the
`render` of the first scene (iso for models), so `classroom_top.scene` gives a
top-down sheet for a top-down game. Views turn the object the same way in
every projection; pivots use the same footprint corners projected by that
camera (in front and side views the footprint is a line, so `front` is the
middle of the bottom edge). Contact shadows are an isometric effect and are
not baked into orthographic sheets.

The output directory gets these files:

- `sheet.png` — transparent grid, one cell per object/view;
- `sheet.json` — engine-neutral manifest;
- `sheet.tsj` — Tiled tileset for **tile layers**, including a Tiled
  `animation` block when a frame row exists;
- `sheet.objects.tsj` — the same tileset for **object layers** (tile objects).

Rows are objects and columns are views, in the order they were loaded. Each
animation gets its own row at the bottom, one column per frame, so a sheet can
combine a 4-view turntable with animated parts. Empty cells stay transparent.

`--animation <object>` may be repeated. `--from`, `--to`, `--frames`,
`--duration`, `--durations`, `--rotation`, `--once` and `--label` apply to the
latest `--animation` (flags given before the first one apply to the first):

```bash
iso-scene sprites door.scene --views 4 \
    --animation door_leaf --label open  --from 0  --to 90 --frames 7 \
    --animation door_leaf --label close --from 90 --to 0  --frames 4 \
        --durations 60,60,60,400 --once \
    -o out/door
```

`--label` names the animation in the manifest (default: the object name;
two animations of one object need distinct labels). `--durations` gives one
duration per frame in milliseconds. `--once` marks a one-shot animation:
`loop: false` in `sheet.json` and a `loop` property on the Tiled tile. Tiled
itself always loops tile animations, so the game decides what to do with it.
At most 1 024 frames in total are allowed.

All cells share one anchor at the horizontal centre of the cell. For a static
view the anchor is a point of the declared footprint `(width, depth)` at floor
level, chosen by `--pivot`:

- `front` (default) — the front corner, the point nearest the viewer that
  touches the floor; Tiled aligns it to the bottom of a tile cell;
- `back`, `left`, `right` — the other corners;
- `center` — the middle of the footprint, handy for inventory icons.

The footprint is used rather than the geometry's bounds, so a table top or a
cornice that overhangs the footprint does not move the anchor. The object is
placed so that the pivot lands on `(cell.width / 2, anchor.y)`. A game draws
the cell at the object's floor position; Tiled uses the same point through
`tileoffset`. For an animation frame the anchor is the object's local origin
instead (`pivot: "origin"`), because that is the hinge the part turns around:
the hinge stays put while the leaf swings. Every sprite and frame in
`sheet.json` names its `pivot`, so a consumer never has to assume one:

```json
{
  "format": "iso-scene-sprites", "version": 1,
  "image": "sheet.png", "imageWidth": 512, "imageHeight": 320,
  "name": "sheet", "projection": "iso", "scale": 32, "tileWidth": 64,
  "views": [0, 90, 180, 270], "pivot": "front",
  "cell": { "width": 128, "height": 160 }, "padding": 0, "shadow": true,
  "anchor": { "x": 64, "y": 132 },
  "columns": 4,
  "rows": 2,
  "sprites": [
    { "name": "chair", "view": 0, "rotation": 0,
      "x": 0, "y": 0, "w": 128, "h": 160,
      "anchor": { "x": 64, "y": 132 }, "pivot": "front",
      "footprint": { "width": 1.8, "depth": 1.8 }, "source": "chair.model" }
  ],
  "animations": [
    { "name": "open", "object": "door_leaf", "from": 0, "to": 90,
      "rotation": 0, "duration": 100, "loop": true,
      "frames": [
        { "name": "door_leaf", "index": 0, "angle": 0, "duration": 100,
          "rotation": 0, "x": 0, "y": 160, "w": 128, "h": 160,
          "anchor": { "x": 64, "y": 132 }, "pivot": "origin",
          "footprint": { "width": 1, "depth": 0.1 }, "source": "door.scene" }
      ] }
  ]
}
```

The manifest is described formally by
[`sprite-sheet.schema.json`](sprite-sheet.schema.json) (JSON Schema 2020-12);
a test keeps the two in sync. `format` and `version` identify it, and
`version` is bumped on incompatible changes. Pixel coordinates start at the
top-left of the image. `rotation` is the yaw of a static view (or the quarter
turn of an animation row); `angle` is the extra yaw of an animation frame.
`index` is the frame's column, so engines can advance frames without parsing
the coordinates. `footprint` is the declared object size in blocks and
`source` the file the object came from.

`--padding <px>` (default 0) leaves transparent pixels around the sheet and
between cells, so linear filtering, mipmaps or scaling cannot bleed one sprite
into its neighbour. The manifest coordinates include it; the tileset writes it
as `margin` and `spacing`.

`--atlas` also writes `sheet.atlas.json` in the TexturePacker "JSON Hash"
format, which Phaser 3 (`this.load.atlas`) and PixiJS (`Spritesheet`) read
directly. Frame keys are `object/view` for static views and `animation#frame`
for animation frames; each frame has a `pivot` (the anchor as a fraction of the
frame), animation frames have a `duration`, and `animations` lists the frame
keys of every animation in order:

```json
{
  "frames": {
    "chair/0": { "frame": { "x": 0, "y": 0, "w": 128, "h": 160 },
                 "rotated": false, "trimmed": false,
                 "spriteSourceSize": { "x": 0, "y": 0, "w": 128, "h": 160 },
                 "sourceSize": { "w": 128, "h": 160 },
                 "pivot": { "x": 0.5, "y": 0.825 } },
    "open#0": { "...": "...", "duration": 100 }
  },
  "animations": { "open": ["open#0", "open#1"] },
  "meta": { "app": "iso-scene", "image": "sheet.png", "format": "RGBA8888",
            "size": { "w": 512, "h": 320 }, "scale": "1" }
}
```

All cells of a sheet share one size, so a single door makes every chair
cell door-sized. `--max-rows <n>` splits the rows into pages of at most `n`
rows — `sheet-1.png`, `sheet-2.png`, … each with its own cell size, anchor
and tileset (`sheet-1.tsj`, …) — while `sheet.json` stays one manifest: its
`pages` array lists every image, and each sprite and frame names its `page`
(its `x`, `y`, `w`, `h` and `anchor` refer to that page). Inputs are packed in
load order, so list small and large objects in separate groups to keep small
cells together. Without `--max-rows` there is one page and the files keep their
plain names.

`--debug-grid` also writes `sheet.debug.png`: the sheet on a light background
with each cell outlined, the anchor marked with a cross and a `name/view`
(`name#frame` for animation frames) label, to check at a glance that nothing is
clipped and the anchors line up.

The tileset uses the same cell size, `columns` columns, and `tileoffset.x =
(tileWidth - cellWidth) / 2` so a tile placed in a tile layer keeps the sprite
bottom-centre on the cell. `tileoffset.y` shifts the image down by the empty
bottom margin, so the anchor (not the cell's bottom edge) lands on the tile
cell. `--tile-width` overrides the tile width used for that offset (by default
`2 * scale`). Tile objects on an isometric map are already bottom-centre aligned
on their position, and Tiled adds `tileoffset` on top, so `sheet.objects.tsj`
uses `tileoffset.x = 0` (same `y`, `objectalignment: bottom`). Both were checked
with Tiled's `tmxrasterizer` on an isometric map: the anchor lands exactly on
the cell's bottom vertex in a tile layer with `sheet.tsj` and for a tile object
placed at that vertex with `sheet.objects.tsj`. The first frame tile carries the `animation` list; every static
tile carries `name`, `rotation`, `view`, `pivot` and anchor properties.
`tilecount` is `columns * rows` of the page, including empty cells (for
example when an animation is longer than the view row): Tiled derives the tile
grid from the image, so the count must match it. Empty cells are transparent
tiles in the Tiled palette and have no entry in `sheet.json`.

Contact shadows are baked into each sprite (`--shadow contact`, the default;
`--shadow none` leaves them out for games that draw their own), like in the
Tiled object atlas, and
keep their partial alpha: sprites are composited with straight-alpha "over",
and each sprite is clipped to its own cell.

## Walls

A `.walls` layout is exported as two Tiled tile layers around `objects`:
`walls-back` below them and `walls-front` (the near walls of each room)
above them. `--walls-behind` and `--walls-front` put a single `walls` layer
below or above the objects instead. The generated tileset has 16 tiles matching the 16
neighbour masks and an edge Wang set named `wall`. Connected sides have no
outline, so the wall reads as one continuous surface. See `docs/walls.md`.

## SVG

`render` writes an SVG document when the output ends with `.svg`
(iso scenes only):

```bash
iso-scene render room.scene -o out/room.svg
```

The faces are the same ones the raster backend uses. Their order is derived
from the same visibility the PNG shows: a depth buffer records which face wins
every pixel, and each meeting of two faces at a pixel becomes an ordering
constraint (the face behind is drawn first). A topological sort turns those
pixel constraints into a painter order, so the vector output covers exactly
what the PNG covers. Outlines are drawn as 1 px polygon strokes.

This matters for small parts in front of a large face: a handle behind a door,
or a jamb in front of a panel, is ordered from the pixels it actually occupies
instead of from a mean depth. Faces that genuinely interleave (in front at one
pixel and behind at another) cannot be ordered by a painter's algorithm; such a
cycle is broken by mean depth.

### Animated sprite sheets

`iso-scene animate scene.scene --object leaf --from 0 --to 90 --frames 7 --duration 100 -o out/door`
renders one frame per angle, packs them into a horizontal sheet and writes a
tileset whose first tile carries a Tiled `animation` block. Open
`out/door/scene.tmj` in Tiled and the tile animates in the editor and in the
game. The rotating instance turns around its local origin, so a hinged part
should be modelled with the hinge at `(0, 0)`.

### Contact shadows

Object sprites written by the Tiled exporter get a baked diamond-shaped contact
shadow under the footprint. It is part of the sprite, so it costs nothing at
runtime and follows the object in Tiled.
