# gcad2glb

A library that turns `.gcad` and `.gasm` files into game assets at runtime (bodies, parts, rigs,
clips, morph targets, LODs, baked normal maps, GLB bytes), and the `gcad2glb` command over it.

```toml
gcad2glb = { git = "https://github.com/v0l/gcad", rev = "<rev>", default-features = false }
```

```rust
let prop = gcad2glb::Assets::from_path("models/mushroom.gcad", &Default::default())?;
```

- default (the command): meshes and solids, through gcad-brep and the monstertruck kernel.
- `default-features = false`: meshes only, about 1 MB in a game, builds for wasm32.
- `brep`: solid parts and `.gasm` assemblies.
- `parallel`: rayon in gcad-mesh.

```
gcad2glb [--detail N] [--z-up] [--tris N] [--max-error E] [--height H | --scale K] [--centre] [--set N=V] [--cache DIR] in.gcad out.glb
gcad2glb key [OPTIONS] in.gcad
gcad2glb bake [--check] [OPTIONS] SRC_DIR OUT_DIR
gcad2glb cache prune DIR [--max-bytes N] [--max-age DAYS] [--keep index.json]
```

How a game uses it, the options, errors, caching, sizes and build times:
[docs/gcad2glb.md](../../docs/gcad2glb.md).
