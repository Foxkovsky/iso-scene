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
- grid size `ceil(extent / step)`, at most 256 per axis and 256³ total;
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

Tiled's isometric screen space shifts maps right by `height * tileWidth / 2`,
and draws a cell's diamond starting half a tile further right. The exporter
adds both offsets, so an object's bottom-center lands on the bottom vertex of
its cell and the sprite covers the cell it stands on. A 0.001 px nudge into the
cell keeps Tiled's own cell lookup (`pixelToTileCoords`) on that cell instead
of rounding to the next one at the shared corner.

The `floor` layer remains a regular tile layer. Because object order is stored
in the object layer, the Tiled `draworder` is `index`; do not sort objects in
the Tiled UI if you want to preserve the generated depth order.

## Walls

A `.walls` layout is exported as a third Tiled layer named `walls`, above
`floor` and `objects`. The generated tileset has 16 tiles matching the 16
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
