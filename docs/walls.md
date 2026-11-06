# Walls

Walls are a separate subsystem and a separate Tiled layer. They are not
furniture objects and do not use the object atlas.

## File format

`room.walls`:

```c
version 1;

height 2;                                  // blocks
thickness 1;                               // block; must be 1
material wall = "materials/wall.toml";     // optional

layout {
    "########################"
    "#......................#"
    "#......................#"
    "########################"
}
```

- `#` — wall;
- `.` — empty;
- `D` — door gap;
- `W` — window gap;
- `D` and `W` do not draw a wall tile; put a door/window object in the scene.

Material paths are relative to the `.walls` file. If no material is given,
the built-in `light` palette is used. `height` is 1..8 blocks.

## Masks and Wang set

Each `#` gets a 4-bit mask:

```
top = 1    right = 2    bottom = 4    left = 8
```

A bit is set when that side is **open** (no neighbouring wall). Tile index is
the mask, 0..15. The generated Tiled tileset contains:

- 16 tiles in one horizontal strip;
- `tilewidth = 2 * scale`, `tileheight = (height + 1) * scale`;
- an edge Wang set named `wall`;
- `wangid = [top, 0, right, 0, bottom, 0, left, 0]`, where 1 means connected.

At `scale = 32`, a 2-block wall is one `64x96` tile: a `64x32` cap and two
64-pixel facades. Connected sides have no outline, so neighbouring wall tiles
form a continuous surface.

## Tiled export

```bash
iso-scene tiled room.scene --walls room.walls -o out/tiled
```

The exported map has three layers, bottom to top:

1. `floor` — tile layer;
2. `walls` — the generated 16-tile wall layer with the `wall` Wang set;
3. `objects` — furniture, doors and windows (a depth-sorted object layer, or a
   tile layer with `--tile-layer`).

Walls are placed behind the objects by default, so a door standing in a wall
gap is not cut by the wall tiles. Pass `--walls-front` to put the wall layer
above the objects instead; that suits cut-away rooms without near walls.

## Current limitations

- walls are one cell thick and full-cell; thickness must be 1;
- doors and windows are gaps plus separate scene objects, not CSG;
- the ISO/ortho/voxel previews render the scene objects only; the generated
  wall layer is currently visible in Tiled, not in `preview.png`;
- wall height is uniform for the whole layout;
- no windows model yet, only `W` gaps.
