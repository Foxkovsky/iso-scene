# Backends

This document fixes the parts of rendering that the language contract depends
on. It will grow with the implementation.

## Built-in materials

v0.1 uses only these palettes. TOML material files are parsed by the language
but loading them into the renderer is the next step; using an alias currently
reports `E0601`.

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
