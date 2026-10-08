# gcad

Parametric CAD written like G-code: one operation per line, built for agents to write
and check.
It is pure Rust on the [monstertruck](https://github.com/v0l/monstertruck) B-rep
kernel and exports STEP, STL, OBJ, 3MF and SVG drawings.

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
cargo run --release -- check examples/parts/plate.gcad
cargo run --release -- render examples/parts/plate.gcad plate.png
cargo run --release -- export examples/parts/plate.gcad plate.step
cargo run --release -- bom examples/assemblies/enclosure.gasm
cargo run --release -- examples/assemblies/enclosure.gasm
cargo run --release -- examples/robot/arm.gasm
```

Part files (`.gcad`) build one or more bodies. Assembly files (`.gasm`) bring parts in
from part files, place them, join them with turning and sliding joints and check
that they do not overlap.

`examples/robot/arm.gasm` is a robot arm built the way industrial arms are: each
joint has a motor on its axis under a round cover, driving a planetary reducer
enclosed in the joint housing. The ring gear is cut into the housing, the sun sits on
the motor shaft, and the three planets turn on pins of the next link, which is the
reducer's output. The gripper's fingers are racks on one pinion inside its housing.
Drag the joint sliders in the viewer and every sun and planet turns at its ratio; the
explode slider and the section tool show the gears inside.

![The robot arm example in the viewer](docs/robot-arm.png)
![A section through the shoulder reducer](docs/robot-arm-section.png)

With no command, `gcad` opens the viewer: a live view of the file that rebuilds on
save, with a view cube, measuring, and sliders for an assembly's joints.

The format, operations and selectors are in [docs/format.md](docs/format.md). Which CAD
operations exist, which are still missing and the tests behind each are in
[docs/ops.md](docs/ops.md).

The kernel is a fork with fillet and boolean fixes, checked out next to this repo at
`../monstertruck` (branch `cad-regressions`) and wired in through `[patch.crates-io]`.
