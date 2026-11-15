# scene-wasm

Minimal WebAssembly entry point for the isometric renderer.

`render_demo()` compiles a tiny built-in scene, renders it through
`scene-backend-iso`, and returns the RGBA bytes to JavaScript. It proves that
the geometry, materials and rasterizer compile to `wasm32-unknown-unknown`
without a filesystem or a server.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --locked
cargo build -p scene-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir ../game/web/wasm \
    target/wasm32-unknown-unknown/release/scene_wasm.wasm
```

The generated JS and `.wasm` are loaded by `game/web/wasm.html`.

## Next

The demo scene is hard-coded. The navigator will pass a serialized scene
(geometry + materials) to the module and render only visible tiles; the
filesystem-dependent `include`/material loading stays on the native side or is
replaced with an in-memory provider.
