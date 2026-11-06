# gcad-mesh: build models from gcad text at startup

`crates/gcad-mesh` is the mesh and rig half of gcad as a small library. It runs the mesh operations
(`blob`, `limb`, `remesh`, `select` and the sculpt ops, `bone`, `skin`, `pose`, `clip`, `key`, plus
`let`, `if`, `body`, `move`, `rotate`, `scale`, `mirror`, `color`) with no CAD kernel, egui, wgpu or
sims. The main `gcad` crate uses the same code, so there is one implementation. The `.gcad` text ships
in the game (a few KB) and the model is built when the game starts, or at build time into a GLB.

Dependencies: `anyhow` and `serde_json` (rayon only with the `parallel` feature). No global state, no
threads unless `parallel` is on, same text gives the same bytes.

## Rust API

```rust
const HERO: &str = include_str!("hero.gcad");

let model = gcad_mesh::build(HERO)?;
let body = &model.bodies[0];
// body.positions, normals, colours (per vertex, f32), indices (u32, grouped by part)
// body.parts: name, colour, gloss, first_index, index_count (one draw call each)
// body.joints, joint_indices, joint_weights (4 influences), body.clips (keyframes)
let walk = body.clip_index("walk");
let matrices = body.bone_matrices(walk, t);
let skinned = body.skinned_positions(&matrices);
let glb: Vec<u8> = model.to_glb()?;
```

`bone_matrices(clip, t)` returns one column-major 4x4 matrix per joint that moves a rest-pose vertex to
the posed one, so an engine can upload them and skin on the GPU with `joint_indices` and
`joint_weights`. `None` for the clip gives the rest pose. `skinned_positions` is the CPU version.
Coordinates are Y up like the GLB; `Options { y_up: false, .. }` keeps gcad's Z up.

`build_with(source, &Options)` takes `detail` (cell size multiplier, 1 is the file's own resolution,
2 halves the grid on each axis) and `vars` (override `let` values). Errors are `gcad_mesh::Error` with
`line`, `text` and `message`; a line outside the mesh subset (`rect`, `extrude`, ...) fails with its
line number instead of being run.

`gcad_mesh::to_glb(&str)` is `build(..)?.to_glb()`. The GLB equals `gcad export f.gcad f.glb` byte for
byte for `creature`, `fox` and `rigged_creature` (tests in `tests/gcad2glb.rs`).

## C ABI

`crates/gcad-mesh/include/gcad_mesh.h`, linked from `libgcad_mesh.a` or `libgcad_mesh.so`. `gcad_build`
returns a handle that is always non-null; `gcad_error` is null on success, else the line numbered
message. Buffer getters return pointers owned by the handle, valid until `gcad_free`.
`gcad_bone_matrices(model, body, clip, t, out, capacity)` fills `joints * 16` floats (clip -1 is the
rest pose) and returns the joint count, also when `out` is null. `gcad_glb` returns bytes released with
`gcad_glb_free`. `crates/gcad-mesh/tests/c_abi.c` is built and run from `tests/c_abi.rs`.

## Command line

```
gcad2glb [--detail N] [--z-up] in.gcad out.glb
```

A `build.rs` step for a game:

```rust
fn main() {
    let spec = std::fs::read_to_string("art/hero.gcad").unwrap();
    let glb = gcad_mesh::to_glb(&spec).unwrap();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("hero.glb"), glb).unwrap();
    println!("cargo:rerun-if-changed=art/hero.gcad");
}
```

On device, call `build` at load and upload the buffers directly. wasm32, iOS and Android targets build
(`cargo build -p gcad-mesh --lib --target wasm32-unknown-unknown`); for Android and iOS use the
`staticlib` or `cdylib` with the C header. `crates/gcad-mesh-viewer` is a macroquad example that builds
`rigged_creature.gcad` at startup and plays the `walk` clip.

## Measured

Linux x86_64, release, one run each, cold start of the process.

| thing | value |
|---|---|
| `gcad2glb` binary | 1.24 MB (1.42 MB with `parallel`) |
| `gcad-mesh` wasm32 library | 674 KB, 223 KB gzipped |
| `libgcad_mesh.so` aarch64-linux-android (cargo-ndk, NDK 28, release) | 1.22 MB |
| `libgcad_mesh.dylib` aarch64-apple-ios (release, built on the M1) | 1.02 MB |
| `libgcad_mesh.a` android / ios | 26.9 MB / 20.1 MB (unstripped archives, the linker keeps only what is used) |
| `gcad2glb` on the M1 (aarch64-apple-darwin) | 1.11 MB, `rigged_creature` in 0.44 s wall, GLB byte-identical to the Linux build |
| crates built for `gcad-mesh` | 7 including itself |
| release build of the crate with dependencies cached | 1.4 s |
| `rigged_creature.gcad` to GLB | 1814 B to 477700 B, 65 ms (16 ms `parallel`, 13 ms at `--detail 2`, 124440 B) |
| `creature.gcad` to GLB | 513 B to 1274984 B, 243 ms |
| `fox.gcad` to GLB | 3564 B to 14948800 B, 5.8 s |
| kids-free-games `art/3d/hero/hero.gcad` to GLB | 12885 B to 471644 B, 474 ms |

Khronos glTF-validator reports 0 errors and 0 warnings for `rigged_creature` and the hero, and three.js
loads `rigged_creature` with its two clips and 14 bones (`tools/gltf`).

The hero GLB in `kids-free-games/assets/models/hero.glb` is 471748 B, so shipping the 12885 B text and
building at load saves 97 percent of the asset bytes.

`tools/gltf/load.mjs GLB clip t expected.json` loads a GLB in three.js, poses it at the clip time and
compares sampled skinned vertices with `cargo run -p gcad-mesh --example dump_skinned`. On
`rigged_creature` walk at 0.37 s, 2160 vertices differ by at most 3.9e-6 (rest displacement up to 2.1).

![viewer frame](gcad-mesh-viewer-a.png)
![viewer frame later](gcad-mesh-viewer-b.png)

`crates/gcad-mesh-viewer` running in a headless sway: the creature is built from `rigged_creature.gcad`
at startup (66 ms) and the walk clip is skinned on the CPU every frame.
