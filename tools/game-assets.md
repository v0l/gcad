# Game asset development commands

Run `game-assets.sh` with
`sh` in a POSIX shell, Git Bash or WSL. It installs nothing. WSL requires its own
Linux build toolchain; a Windows Rust installation is not a Linux compiler.

```sh
cargo build --locked --features game-assets
sh tools/game-assets.sh prepare-fixtures
```

Run `prepare-fixtures` before `cargo test --features game-assets`: the viewer tests in
`src/view/asset_*` read weight files it writes under `target/` (for example
`target/game-asset-plate.v2.weights.json`) and fail without them. Bindings record the gcad
version and the CAD source hashes, so after changing either, move the old `target/*.weights.json`
and `target/*.faces.json` aside and prepare them again with the build under test, or the tests
fail with "CAD inputs changed; explicit rebinding is required". With a shared cargo target
directory the files still go to this checkout's `target/`.

Existing weights and face selections are preserved. Set `GCAD=/path/to/gcad` to select an executable.

`sh tools/game-assets-lod-fixture.sh NEW_DIRECTORY` reproduces the refined skinned
LOD experiment after `prepare-fixtures`. It retains the original GLB, writes the
derived GLB and captures its JSON report with explicit fixture error limits.
The accepted fixture recipe uses a 1 mm endpoint-correspondence allowance while
enforcing sampled surface maximum 0.1 mm, p95 0.05 mm, silhouette difference
0.01, UV difference 0.02 and shading 3 degrees. Endpoint correspondence is a
conservative internal constraint, distinct from closest-surface error.
Inspect `target_reached` as well as measured errors: a valid partial reduction
does not establish the requested triangle target. This recipe is not a production
performance benchmark or an engine acceptance claim.

Engine adapters, the FBX helper, the renderer patches and the GPU verification scripts live in the private repo v0l/gcad-engines. They call a gcad binary through `GCAD`.
