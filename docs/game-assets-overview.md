# Game assets in gcad: overview

> Game-engine integrations live in the private repo v0l/gcad-engines: the Bevy loader `gcad-bevy` (built on `gcad2glb`), the Unity package `io.v0l.gcad`, the Godot addon, the FBX helper and the renderer patches. Paths written as `gcad-engines:...` below refer to that repo; its `docs/status.md` says what has been run where. The `consumer unity version=1` profile and the `.gasset` format stay here.

gcad has two ways to make a game asset. Both end in a GLB.

- **Mesh files** (`.gcad` with `blob`, `limb`, brushes, `bone`, `skin`, `clip`, `key`) are built by
  the `gcad-mesh` crate. `gcad export f.gcad f.glb` and the standalone `gcad2glb` write the same
  bytes. This is in every build and is the path for characters, creatures and props sculpted from
  blobs.
- **Asset files** (`.gasset`) come from [Wixely's game-assets fork](https://github.com/Wixely/gcad/tree/game-assets),
  merged with its history. A `.gasset` takes bodies from `.gcad` or `.gasm` CAD files and adds
  tessellation, UVs, textures and atlases, a node tree, skins bound from weight files, clips, root
  motion, LODs, collision shapes, sprite sheets and an experimental FBX package. It is
  **experimental** and only in builds with the `game-assets` feature. Its guide is
  [game-assets.md](game-assets.md); sprites, collision and the consumer profile have their own pages.

## Building with the asset tools

```sh
cargo build --release --features game-assets
gcad check examples/game-assets/plate.gasset
gcad export examples/game-assets/plate.gasset plate.glb
gcad examples/game-assets/plate-collision.gasset
```

The default build has none of it: no `asset-*` commands, no `.gasset` in the viewer, no xatlas.
Several examples need weight files made first by `sh tools/game-assets.sh prepare-fixtures`
(set `GCAD=target/release/gcad`). FBX output needs a native helper built from pinned Assimp and
ufbx sources (`gcad-engines:fbx-helper`), and the Unity and Godot importers in
`gcad-engines:unity` and `gcad-engines:godot` need those engines.

The fork's renderer changes (explicit mip chains, material alpha, occlusion and emission maps, UV
transforms) live in egui_bench. gcad pins `v0l/egui_bench` branch `gcad-renderer`, which is
egui_bench master (0.9.6, per-vertex colours) merged with Wixely's `gcad-renderer` branch, through
`[patch.crates-io]` in `Cargo.toml`. All builds use it; with default values it renders as before.

## Comparison

| capability | fork (`game-assets` feature) | gcad master | kids-free-games script | decision |
|---|---|---|---|---|
| GLB writer | `crates/gcad-view/src/asset/glb.rs`: textured PBR, tangents, node tree, skins, clips, root motion, extras for collision and LOD | `gcad-mesh/src/gltf.rs`: vertex colour materials per part, skin, clips; `crates/gcad-drafting/src/export.rs` for solid parts | none | keep both. Mesh files keep the `gcad-mesh` writer so `gcad2glb` stays light and byte-identical; `.gasset` keeps its own. Unify later (#266) |
| scale to game units | none (always mm to m) | `gcad export --height H --scale K --centre`, `gcad2glb` the same | `scale_glb.py` | absorbed. Same `model` root node and bytes as the script |
| props with named parts and colours | `.gasset` nodes with `surface` per source | `gcad export` of a multi-body `.gcad`: one node per `body`, one material per colour | `props/build.sh` | already covered by `gcad export`; the coin prop gives the same GLB as `build.sh` |
| mocap to clips | none (clips are keyed by hand, or baked from assembly joints) | `clip` and `key` lines | `hero/retarget.py`, `clips.json` | absorbed as `tools/bvh-retarget` with a JSON config; the hero config reproduces its `motion.gcad` byte for byte |
| turntable and pose views | viewer turntable and sprite sheets (lit, albedo, normals) for `.gasset` | `gcad render --turns N --clip C --times ...` | `render_views.py` (Blender) | absorbed for review sheets. Game sprite sheets: use the fork's sprite export |
| skin weights and rig | `crates/gcad-view/src/asset/skin.rs`: weight files bound to a stamped mesh, palettes, quantise, prune; viewer weight painting by face | `gcad-mesh` `bone`, `skin auto\|part\|paint`, `pose` | rigid part binding in `hero.gcad` | one deformation path: a `.gasset` takes a mesh-body source with its `bone`/`skin`/`clip` lines and binds them as a fork skin, so prune, quantise, collision, LOD and FBX apply. Weight files stay for tessellated CAD |
| LOD and reduction | `asset-lod` (error-bounded, UV and shading checks, normal bake), `asset-reduce` (clip keys), viewer LOD comparison | `finish decimate=`/`max_error=`, `export --tris`, `Mesh::cluster` viewer edit LOD | Blender decimate in `prop_build.py` | keep both: ours for sculpted mesh bodies, theirs for tessellated CAD with UVs. `asset-lod` and `finish` share one quadric (`gcad_mesh::mesh::quadric`); see "Collapse ordering" below |
| UVs and textures | box and xatlas unwrap, UV edit by face, atlas packing with mips, paint guide and apply, texel density | procedural surface patterns in the viewer (`textures.rs`), now from `model::texture::pattern` | Hunyuan3D textures (`props.py`) | the pattern code is shared (the fork moved it to `model/texture.rs`). Painted UV textures exist only for `.gasset` |
| viewer animation | `.gasset` clip timeline, rig overlay, weight view | animation card for mesh clips (`crates/gcad-view/src/view/anim.rs`) | none | keep both; each drives its own document type (#269) |
| collision shapes | `collision NODE shape=...` in GLB extras | none | none | fork only |
| FBX | experimental helper process and package | none | none | fork only, needs the native helper |
| GLB audit (tris, bones, texture size) | `asset-report`, `asset-capabilities` | `gcad check` prints triangles per body | `scripts/glb_audit.py` | the game keeps its own budget check |
| image to 3D | none | none | `props.py` (Hunyuan3D) | out of scope for gcad |

## Which to use

A character or creature sculpted from blobs: a mesh `.gcad`, rigged with `bone` and `skin`,
animated with `clip` lines or `tools/bvh-retarget`, exported with `gcad export f.gcad f.glb
--height 1` or `gcad2glb`. Review it with `gcad render f.gcad sheet.png --turns 8 --clip run`.

A prop from CAD solids, one colour per body: a `.gcad` part, `gcad export --height 1 --centre`.

A textured asset from CAD parts or assemblies, with UV painting, atlases, collision, LODs, sprite
sheets or FBX: a `.gasset` in a `game-assets` build.

## Collapse ordering

`asset-lod` ranks edge collapses by the shared quadric error instead of edge length. Every gate
(position, UV, normal, posed, topology) is unchanged, so ordering only decides which collapses are
tried first. `--edge-length-order` restores the old ranking. Measured on the fork's fixtures with
the fork's own report (worst value over all sampled poses, both directions):

| Fixture, target | Order | Triangles | Target met | Correspondence mm | Surface max mm | Surface p95 mm | Silhouette diff | Shading angle |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `plate-lod-bake`, 3000 | edge length | 3000 | yes | 1.000 | 0.1862 | 0.0431 | 0.00139 | 26.77 |
| `plate-lod-bake`, 3000 | quadric | 3000 | yes | 1.000 | 0.0868 | 0.0230 | 0.00035 | 24.20 |
| `plate-scene-refined`, 14000 (fixture script, 5000 trials) | edge length | 14000 | yes | 0.441 | 0.0030 | 0.0000 | 0.00000 | 2.93 |
| `plate-scene-refined`, 14000 (5000 trials) | quadric | 14660 | no, trial limit | 0.928 | 0.0047 | 0.0000 | 0.00028 | 2.97 |
| `plate-scene-refined`, 14000 (20000 trials, fixture limits) | quadric | 14000 | yes | 0.928 | 0.0047 | 0.0001 | 0.00028 | 2.97 |
| `plate-skinned`, 600 | edge length | 1054 | no | 0.987 | 0.2185 | 0.0608 | 0.00170 | 25.12 |
| `plate-skinned`, 600 | quadric | 1056 | no | 0.975 | 0.2185 | 0.0422 | 0.00119 | 25.12 |

Quadric order halves the surface error and silhouette difference on the bake plate, is neutral on
the skinned plate (UV limits block both at about 1055 triangles) and needs about 2.1x the trials
on the 17.5k-triangle refined plate to reach 14000, though that costs less wall time (55 s
against 76 s) because most trials die before the expensive pose checks. Surface error stays far
inside the fixture limits (0.1 mm max, 0.05 mm p95, 0.01 silhouette), so quality is not worse and
quadric order is the default. Raise `--max-trials` for large meshes.

`Mesh::finish` already uses the same quadric code, so there is no second decimator. Wrapping
`finish` in the fork's acceptance checks is not done: the checks need the skinned pose samples
that `finish` does not have.
