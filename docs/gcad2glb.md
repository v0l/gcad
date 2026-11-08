# gcad2glb, the library

`gcad2glb` is a library that turns `.gcad` and `.gasm` files into game assets while the game runs,
or in its `build.rs`. It is also the `gcad2glb` command, which is a few lines over the library and
writes the same bytes as `gcad export f.gcad f.glb`.

The crate keeps the name `gcad2glb` and is a library and a binary in one package
(`src/lib.rs`, `src/main.rs`). Nothing else is called that, the user and the docs already call
the tool that, and a separate `gcad-assets` crate would be one more name for the same code (and
would collide with the `game-assets` feature and `gcad::asset`).

For Bevy, `gcad-engines:bevy` (the `gcad-bevy` crate) wraps this library in an asset loader:
`.gcad`/`.gasm` load as scenes with skins, clips, morph targets and LODs. See
`gcad-engines:docs/bevy.md`.

## Depend on it

```toml
gcad2glb = { git = "https://github.com/v0l/gcad", rev = "<rev>", default-features = false }
```

That is the mesh-only build: meshes, sculpts, rigs, skins, clips, correctives, bakes and LODs,
with gcad-mesh, gcad-parse, anyhow, serde_json and miniz_oxide and no CAD kernel. It builds for
`wasm32-unknown-unknown`. Add `features = ["brep"]` for files with solid operations (`hole`,
`fillet`, `plane plate.end`, ...) and for `.gasm` assemblies; that pulls in gcad-brep and the
monstertruck kernel, so copy gcad's `[patch.crates-io]` monstertruck lines into the game's root
`Cargo.toml`. `features = ["parallel"]` turns on rayon in gcad-mesh. The command keeps `brep` on
by default.

Without `brep`, a solid file fails with the first solid line and says the library was built
without the brep feature. Flat solids (`plane`, `rect`, `circle`, `poly`, `array`, `extrude`,
`fillet`, `repeat`) still build without it, through gcad-mesh's own flat extruder, which
tessellates differently from the kernel: the shape is the same, the triangles and bytes are not.

## Use it

```rust
use gcad2glb::{Assets, Fit, Options};

fn mushroom() -> Result<(), gcad2glb::Error> {
    let source = "let cap=6\nstem: blob 0,0,4 r=2,2,5\ncap: blob 0,0,10 r=cap,cap,3 blend=1\n\
                  remesh 0.4\ncolor #e8dcc0\ncolor #c0392b part=cap\n";
    let fit = Fit { height: Some(1.0), ..Default::default() };
    let options = Options { fit, ..Options::default().set("cap", 8.0) };
    let prop = Assets::from_source(source, &options)?;
    for body in &prop.bodies {
        for part in &body.parts {
            let range = part.first_index..part.first_index + part.index_count;
            for t in body.indices[range].chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|k| body.positions[t[k] as usize]);
                let _ = (a, b, c, part.colour, part.gloss);
            }
        }
    }
    let placed = prop.fit().expect("asked for a height");
    println!("scale {} offset {:?}", placed.scale, placed.translation);
    Ok(())
}
```

`crates/gcad2glb/examples/prop.rs` is the same, run with
`cargo run -p gcad2glb --no-default-features --example prop`.

- `Assets::from_source(text, &options)` builds text. With `brep`, a file with any solid line
  (flat `rect`/`circle`/`extrude` included) runs through gcad-brep, which runs solid and mesh
  bodies together exactly as `gcad export` does; a file of mesh lines, or any file without
  `brep`, goes through gcad-mesh. Text has no folder, so `part` lines and stamps that name other
  files need `from_path`.
- `Assets::from_path(path, &options)` reads a file; `part` lines and stamps are relative to it. `.gasm` assemblies always go through gcad-brep and give one body per placed
  part. `.gsim` files are refused.
- `Assets` derefs to `gcad_mesh::Model`, so code written for `gcad_mesh::build_with` (the
  kids-free-games `mesh_of`, `material_of`, `clip_of`) takes it unchanged. A solid body is a
  `Body` with one part per solid, smooth-shaded positions and normals from the same
  tessellation `gcad export` uses, and no joints.
- Each `Body` has `positions`, `normals`, `colours` (per-vertex paint, may be empty), `indices`,
  `parts` (name, colour in sRGB, gloss, index range), `joints`, `joint_indices` and
  `joint_weights` (4 influences), `clips` (rotation `tracks`, translation `moves` and, for
  correctives, a `weights` track), `targets` (morph target deltas) with their rest
  `target_weights`, `bone_matrices(clip, t)` and `skinned_positions`. `lods()` gives the `lod`
  levels as bodies named `<body>_LOD1`, ...; `normal_map()` gives the baked tangent-space PNG
  with the UV-split vertices, tangents and indices it belongs to.
- `bounds()` is the box of every position, `triangles()` the count. `fit()` is the transform
  `Options::fit` asks for (scale, then translation, lowest point at 0, `centre` puts the middle
  on the vertical axis); it is what `gcad2glb --height` writes as the root `model` node.
- `to_glb()` writes the GLB, byte for byte what `gcad export` and the command write.
- `notes` holds what the triangle budget reports per body (the command prints them on stderr).

`Options`:

| field | meaning | command flag |
|---|---|---|
| `detail` | remesh cell scale, above 1 is coarser (mesh-only files) | `--detail` |
| `y_up` | glTF axes, false keeps gcad's Z up | `--z-up` |
| `vars` (`set(name, value)`) | override `let` values, as `gcad --set name=value` | |
| `triangles`, `max_error` | decimate every body at the end | `--tris`, `--max-error` |
| `fit` | height or scale, and centre | `--height`, `--scale`, `--centre` |
| `lods` | replace the file's `lod` levels (`Some(vec![])` drops them) | |
| `texture_size` | replace the size of a `bake normals` map | |

## Errors

One type, `gcad2glb::Error` (it is `gcad_mesh::Error`): `line` (0 when it is about the whole
file), `text` (the source line, up to 80 characters) and `message`. `Display` prints
``line 5 `hole 6 -20,0 20,0`: ...`` as the command does.

## Caching

Building is pure: the same text and options give the same model and bytes. `cache_key(source,
&options)` is a 64-bit FNV-1a hash of the gcad version (`gcad2glb::VERSION`, the package version
plus the last code commit), the text and every option. It is stable across runs and machines.

The key every engine uses is `Sources::key`: `Sources::from_path(file)` (or `Sources::read(name,
text, read)` for a game's own asset reader) follows `include` lines and the files the text names
(`part x.gcad`, stamps, fonts), and the key is `cache_key` of the expanded text folded with the
path and bytes of each of those files. A build runs only when the source, an include, a named
file, the options or the gcad version changed.

`AssetCache::new(dir)` keeps `<dir>/<key>.glb` (the bytes `to_glb` writes) with a `<key>.json`
sidecar (gcad version, source, options hash, the hash of each read file). Writes go to a temp file
and are renamed, so processes racing on one key leave one whole file. A file that is not a whole
GLB is removed and rebuilt. A read touches the file's mtime; `prune(max_bytes, max_age, keep)`
removes entries older than `max_age` or past `max_bytes` (newest kept first), never one in
`keep`. `cache::bake(src, out, options_for)` builds every `.gcad`/`.gasm` under `src` that is
not only included by another into `out/<key>.glb` plus `out/index.json` (asset path to key and
options hash), and `cache::check` lists sources with no entry or a stale one.

```
gcad2glb --cache DIR in.gcad out.glb            # hit: copy, no build
gcad2glb key [OPTIONS] in.gcad                  # print the key
gcad2glb bake [--check] [OPTIONS] SRC_DIR OUT_DIR
gcad2glb cache prune DIR [--max-bytes N] [--max-age DAYS] [--keep index.json]
```

## Size and build time

Linux x86_64, release, `CARGO_BUILD_JOBS=6`, `nice -n 15`, measured with a one-line game binary
that calls `Assets::from_source` (`/tmp/gcad-consumer`, not kept).

| configuration | binary | stripped | added to the game | clean build | touch `gcad2glb/src/lib.rs` |
|---|---|---|---|---|---|
| without gcad2glb | 4.54 MB | 0.34 MB | | | |
| `default-features = false` | 5.95 MB | 1.39 MB | 1.41 MB (1.06 MB stripped) | 5.9 s | 0.4 s |
| `features = ["brep"]` | 22.8 MB | 13.3 MB | 18.3 MB (13.0 MB stripped) | 42.4 s | 0.6 s |

`crates/gcad2glb/tests/dependencies.rs` checks that the mesh-only tree has no monstertruck,
gcad-brep, image, egui, wgpu or solvers, and runs
`cargo check -p gcad2glb --lib --no-default-features --target wasm32-unknown-unknown`.
