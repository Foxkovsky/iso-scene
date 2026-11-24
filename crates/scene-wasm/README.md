# scene-wasm

Minimal WebAssembly entry point for the isometric renderer.

`render_demo()` renders a tiny built-in box. `render_scene(source, base)`
renders a real `.scene` against a virtual file tree filled from JavaScript:
`set_file(path, bytes)` supplies models, materials and PNG textures, and
`clear_files()` resets the tree. Includes, material TOML and decals therefore
work in the browser without a server-side filesystem.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --locked
cargo build -p scene-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir ../game/web/wasm \
    target/wasm32-unknown-unknown/release/scene_wasm.wasm
```

The generated JS and `.wasm` are loaded by `game/web/wasm.html`. The page lets
you pick a project folder (for the university: `content/university`); it skips
`out/`, loads `.model`/`.scene`/`.toml`/`.png`, fills the virtual tree, lists
the scenes and renders the selected one.

## Next

The navigator can use the same `set_file`/`render_scene` API to render a floor
without pre-baked PNG maps. The page currently renders the whole selected scene
at once; visible-tile culling and animation frames are the next step.
