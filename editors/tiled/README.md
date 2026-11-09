# Tiled extension

`iso-scene.js` checks maps written by `iso-scene tiled` for collisions: two
objects whose footprints and height ranges overlap, or an object on a wall
cell. It uses the same rules as the exporter, but on the current object
positions, so it also catches objects moved by hand.

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

- tile objects in the `objects` layer with `footprint_x`, `footprint_y`
  (cells, relative to the object position), `footprint_w`, `footprint_h`,
  `bottom` and `top`;
- wall tile layers named `walls`, `walls-back` or `walls-front`.
