# Refactor baseline

Recorded on master f541e8e (hashes refreshed on d7dbcb1, which changed the two skin examples) before the #278 crate split. Every stage must reproduce
`hashes.txt`, `hashes-game-assets.txt` and `tests.txt`.

```
CARGO_TARGET_DIR=... cargo build --release --workspace
tools/baseline.sh $CARGO_TARGET_DIR/release/gcad /tmp/hashes.txt
cargo build --release --features game-assets --bin gcad
tools/baseline.sh $CARGO_TARGET_DIR/release/gcad /tmp/hashes-ga.txt game-assets
cargo test --release --workspace --no-fail-fast 2>&1 | tee /tmp/test.log
tools/test-lines.sh /tmp/test.log | diff tests/baseline/tests.txt -
```

Each hash line is `hash exit-status kind file`. It covers `check`, STL, OBJ, STEP,
3MF, SVG, DXF, GLB, `render` PNG and `bom` of every `.gcad`/`.gasm` under examples/
and tests/fixtures, `sim --dry-run` of every `.gsim`, `sim-job` of the packed jobs,
and with game-assets `check` and GLB of every `.gasset`. STEP timestamps, SVG line
order (it varies run to run), timings in printed text and the scratch path are
normalised away. Two runs of each binary gave identical files.

`hashes-game-assets.txt` is recorded after `sh tools/game-assets.sh prepare-fixtures`: about 30
`.gasset` files read weight and face files it writes under `target/`, and fail without them.
The binary built from f541e8e gives the same hashes for all of them.

`tests/revision.rs` commits to a scratch repository and fails when git signs commits
and cannot prompt; run the suite with `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=commit.gpgsign
GIT_CONFIG_VALUE_0=false`.

## Timings (48 cores, CARGO_BUILD_JOBS=6, nice 15, release)

| build | seconds |
|---|---|
| clean `cargo build --release --workspace` | 125 |
| clean `--features game-assets --bin gcad` | 158 |
| touch src/sim/mod.rs | 30.9 |
| touch src/model/mod.rs | 28.2 |
| touch src/view/mod.rs | 28.8 |
| touch src/calc/mod.rs | 29.9 |
| touch crates/gcad-mesh/src/parse.rs | 36.3 |
| `cargo test --release --workspace` (build + run) | 472 (60 of it the revision test timing out) |

Slowest units in the clean build: gcad 27.5 s, monstertruck-io 23.9, faer 22.9,
naga 18.2, wgpu-core 17.4, egui 16.6, image 15.9, lopdf 15.0, sims-fem 14.3,
gcad-mesh 12.8.

Binary sizes (release): gcad 61.2 MB, gcad with game-assets 65.6 MB, gcad2glb 2.3 MB.
