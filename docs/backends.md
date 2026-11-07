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

- `scene.tmj` — isometric map, layers `floor` and `objects`;
- `floor.tsj` + `floor_tile.png` — one diamond floor tile;
- `objects.tsj` + `objects.png` — atlas of unique object/rotation sprites;
- `preview.png` — the normal iso render.

If the map looks semi-transparent when opened in Tiled, that is the editor,
not the export: **View → Highlight Current Layer** draws layers above the
selected one at 40% opacity and darkens the layers below. The exported map
always has `opacity = 1` on every layer and only binary alpha in the PNGs.

Objects are written as tile objects in an `objectgroup` layer, ordered by
depth (`x + width + y + depth + z`, far to near). This allows several objects
in one cell and objects above the floor. The tileset has no `tileoffset`; each
object is positioned by its sprite anchor and the map's isometric origin.

Tiled's isometric screen space shifts maps right by `height * tileWidth / 2`;
object coordinates carry that shift. A 0.001 px nudge into the cell keeps
Tiled's own cell lookup (`pixelToTileCoords`) on that cell instead of rounding
to the next one at the shared corner.

### Objects as tiles

`--tile-layer` writes the sprites into a tile layer named `objects` instead of
an object layer: one tile per cell, in depth order. Tiled draws a tile image
with its bottom-left at the cell's position, so the object tileset gets a
`tileoffset` of `(tileWidth - atlasWidth) / 2`. With that offset the sprite's
bottom-center lands on the cell's bottom vertex, and a tile placed by hand in
Tiled covers the cell it stands on. Cells are limited to one sprite each in
this mode.

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
contributes all objects defined in it. Use `--object <name>` (repeatable) to
keep only some of them. The pixel scale is taken from the first scene; models
and mixed inputs use `--scale` (default 16).

`--views 1|2|4` selects the static sides: `0°`, `0°/180°`, or
`0°/90°/180°/270°`. `--angles 0,90,180,270` picks them explicitly.

The output directory gets three files:

- `sheet.png` — transparent grid, one cell per object/view;
- `sheet.json` — engine-neutral manifest;
- `sheet.tsj` — Tiled tileset, including a Tiled `animation` block when a frame
  row exists.

Rows are objects and columns are views, in the order they were loaded. An
animation gets its own row at the bottom, one column per frame, so a sheet can
combine a 4-view turntable with an animated part. Empty cells stay transparent.

Every cell is anchored at its **bottom-centre**. For a static view the anchor
is the footprint's front corner, the point that touches the floor; the object
is placed so that this point lands on `(cell.width / 2, anchor.y)`. A game
draws the cell at the object's floor position; Tiled uses the same point
through `tileoffset`. For an animation frame the anchor is the object's local
origin instead, because that is the hinge the part turns around: the pivot
stays put while the leaf swings. `sheet.json` describes the anchor explicitly:

```json
{
  "image": "sheet.png",
  "cell": { "width": 128, "height": 160 },
  "anchor": { "x": 64, "y": 132 },
  "columns": 4,
  "rows": 2,
  "sprites": [
    { "name": "chair", "view": 0, "rotation": 0,
      "x": 0, "y": 0, "w": 128, "h": 160,
      "anchor": { "x": 64, "y": 132 } }
  ],
  "animations": [
    { "name": "door_leaf", "from": 0, "to": 90, "duration": 100,
      "frames": [
        { "index": 0, "angle": 0, "duration": 100,
          "x": 0, "y": 160, "w": 128, "h": 160,
          "anchor": { "x": 64, "y": 132 } }
      ] }
  ]
}
```

`rotation` is the quarter turn of a static view; `angle` is the extra yaw of an
animation frame. `index` is the frame's column, so engines can advance frames
without parsing the coordinates.

The tileset uses the same cell size, `columns` columns, and `tileoffset.x =
(tileWidth - cellWidth) / 2` so a tile placed in a tile layer keeps the sprite
bottom-centre on the cell. `tileoffset.y` shifts the image down by the empty
bottom margin, so the anchor (not the cell's bottom edge) lands on the tile
cell. `--tile-width` overrides the tile width used for that offset (by default
`2 * scale`). The first frame tile carries the `animation` list; every static
tile carries `name`, `rotation`, `view` and anchor properties.

Contact shadows are baked into each sprite, like in the Tiled object atlas.

## Walls

A `.walls` layout is exported as a Tiled layer named `walls`, above `floor`
and below `objects` (unless `--walls-front` is given). The generated tileset has 16 tiles matching the 16
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
