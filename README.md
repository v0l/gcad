# linecad

Parametric CAD written one operation per line, built for agents to write and check.
It is pure Rust on the [monstertruck](https://github.com/v0l/monstertruck) B-rep
kernel and exports STEP and STL.

```
let w=40 h=30 t=3
rect w h r=4
base: extrude t
plane base.end
mounts: hole 3.2 15,10 -15,10 -15,-10 15,-10
fillet 0.8 base.end&base.side
chamfer 0.3 mounts.side&base.end
```

```
cargo run --release -- check examples/parts/plate.lcad
cargo run --release -- render examples/parts/plate.lcad plate.png
cargo run --release -- export examples/parts/plate.lcad plate.step
cargo run --release -- view examples/parts/plate.lcad
```

The format, operations and selectors are in [docs/format.md](docs/format.md). Which CAD
operations exist, which are still missing and the tests behind each are in
[docs/ops.md](docs/ops.md).

The kernel is a fork with fillet and boolean fixes, checked out next to this repo at
`../monstertruck` (branch `cad-regressions`) and wired in through `[patch.crates-io]`.
