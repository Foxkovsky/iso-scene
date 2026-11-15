# Tiled extension

`iso-scene.js` checks maps written by `iso-scene tiled` for collisions: two
objects whose footprints and height ranges overlap, or an object on a wall
cell. It uses the same rules as the exporter, but on the current object
positions, so it also catches objects moved by hand.

It also reads game maps whose furniture is a **tile layer**: tiles with
`collides: true` and `cells_x`/`cells_y` are expanded to footprints, and
`walls` / `walls-back` / `walls-front` tiles block them. Door tiles with
`collides: false` stay passable. The university game ships a Tiled project
that loads this extension automatically.

## Install

Copy `iso-scene.js` into the Tiled extensions folder (**Edit → Preferences →
Plugins → Open extensions folder**) and restart Tiled. Then **Map → Check
iso-scene Collisions**:

- selects the colliding objects;
- lists each collision in the Console;
- redraws the hidden `collision` layer from the current footprints.

## Without the editor

```bash
tiled --evaluate editors/tiled/iso-scene.js out/tiled/scene.tmj
```

prints the collisions of each map given, for scripts and CI.

## Data it reads

- object-layer maps: tile objects in the `objects` layer with `footprint_x`,
  `footprint_y` (cells, relative to the object position), `footprint_w`,
  `footprint_h`, `bottom` and `top`;
- tile-layer game maps: tiles with `collides: true`, `cells_x`, `cells_y`;
- wall tile layers named `walls`, `walls-back`/`walls-front` or
  `walls_back`/`walls_front`; a tile with `collides: false` (a door) is
  passable.
