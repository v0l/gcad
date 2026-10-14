# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## Five board STEP bodies still come in as boxes

All fail the kernel's healing (`monstertruck-healing`), so they need fork work. In
`~/git/rotator/board/models/Ag5400_simplified.step` (123 bodies written as voids of one pin),
three bodies are "not oriented and closed" and one has a wire that "is not simple". Part 2 of
`~/git/rotator/board/models/V8BR-1AX1-GH.stp` (the RJ45) has a wire that "is not simple". Check
with `part m <file>` in a `.gasm`; OpenCascade reads all of them.

## Each `interference` line tessellates every part again

`sweep_overlaps` (src/model/assembly.rs) calls `geometry::meshed_all` on all 151 rotator parts for
each `interference` line: 2.8 s and 4.1 s of the 19 s `rotator.gasm` takes, against 1.5 s and
1.2 s for the sweeps themselves. Keep the meshes on the model between lines (they only move with
the joints). Repro: `gcad sim ~/git/rotator/sims/torque.gsim` and read the progress lines.

## Streamlined blades read badly in `sim flow`

`examples/flow/caradonna/hover.gsim` (Caradonna-Tung rotor, measured CT 0.00459, 507 N) reads
309 N and about 3x the torque at 13 mm cells, and its wake opens into a cone instead of
contracting to 0.78 R (details in `docs/flow.md`, Hover rotor). Smagorinsky puts eddy
viscosity in the cells touching a wall, a skin friction near 0.04 against 0.003: replace it
with a model that vanishes at walls (WALE or Vreman, `src/flow/lattice.rs` shader) or add a
wall function, then recheck `examples/flow/cube.gsim`, the heatsink and
`flow::square_cylinder_re22000`. Scaling it by 0.1 at the wall already moved 20 mm cells from
110 N, 294 N·m to 173 N, 92 N·m; 0 diverges.

## Uniform lattice is too coarse for rotors

The same rotor gets 15 cells on the chord and under 2 through the blade at 13 mm, and thrust
went 110 N at 20 mm to 309 N at 13 mm without settling. Rerun `hover.gsim` at `mesh 6.5` once the
GPU is free (vLLM held 78 of 96 GB) to see where it converges; the lasting fix is finer cells
near moving parts (local grid refinement in `src/flow/lattice.rs`).

## Fan benchmarks still to match

After the two items above: the EAA FAN-01 axial fan in a short duct (geometry and LDA data at
zenodo.org/records/10787093), and the fan wake against an OpenFOAM sliding-mesh (AMI) transient
run instead of the frozen rotor in `tools/foam/fan.sh`. `examples/flow/fan.gsim` torque and
thrust are unverified until then.

## Magnetics not built yet

Planned in `docs/magnetic.md`: `phase`/`drive` lines (sinusoidal phase currents that follow
the rotor through a sweep) and a `kt` reading; force on a sliding part (weighted Maxwell
stress); eddy and core losses (2D time-harmonic A_z, Steinmetz/Bertotti), feeding `heat`; 3D
magnetics on the tet mesh checked against TEAM problem 13.

## A spinning part cannot carry heat

`spin` with a `heat` line bails in `src/sim/wind.rs`: rotor cells move, so the per-cell
conductivity table in the thermal lattice would have to move with them.

## `slide` on a small curved face holds every direction

A support's held directions come from the triangle normals around each node, kept when they
differ by more than 0.05 after Gram-Schmidt. On a Ø5 bore the normals splay enough that the
axial direction is kept too, so `slide drive.shaft:dbore.side` in `~/git/rotator/sims/azdrive.gsim`
reacted 495 N of worm thrust along the shaft. Use the face's own normal (the cylinder's radial
direction at the node) for curved faces instead of the triangles'.

## Contact on a slip-fit D-shaft never settles

`contact drive.motor drive.shaft` in the same sim ran 30 rounds, 4 minutes each, with the same
2345 pairs touching and the residual never reaching 1e-9. The D-flat is the shaft's only hold
against turning, so each round's linear solve is near-singular about the axis.

## Cutting thin repeated sheet profiles fails depending on where the rows sit

A box-profile cladding sheet (a 0.5 mm closed `poly` of one 200 mm rib, extruded, then
`repeat count=2 step=0,200`) cut by a raked `poly` with `cut thru` fails with "invalid output
shell for `normalized boolean`: no boundary shells" for most row heights and works for a few:

```
let z0=2265
plane YZ offset=330
poly -38,z0 -38,z0+60 -69.5,z0+80 -69.5,z0+120 -38,z0+140 -38,z0+200 -38.5,z0+200 -38.5,z0+140 -70,z0+120 -70,z0+80 -38.5,z0+60 -38.5,z0
rib: extrude 4320
repeat rib count=2 step=0,200
plane XZ offset=100
poly 320,2596 4660,2488 4660,3500 320,3500
cut thru
```

z0=2245 and 2255 cut; 2265, 2275 and 2300 fail. The cutting line crosses the thin faces at a
shallow angle and nowhere near a rib joint, so it is not a coincident face. The same family
broke opening cuts and `split` on the shed cladding (`~/git/office-shed/parts/panel.gcad`,
`endwall.gcad`, `portal.gcad`), which now dodge it by building strips around openings and by
nudging row heights until the cut goes through.

## Assembly exports lose the parts

`gcad export ~/git/office-shed/shed.gasm x.step` writes one unnamed `PRODUCT` with 167 solids
while the assembly has 266 bodies (the weldment members look to be missing), so OpenCascade
sees no assembly tree and no names. `x.3mf` keeps 266 objects but names them `body0`,
`body1`, ..., so nothing outside gcad can tell which part is which. Name 3MF objects after
the bodies and check `export::assembly_step` against an assembly whose parts include members.
