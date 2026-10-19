# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## Five board STEP bodies still come in as boxes

All fail the kernel's healing (`monstertruck-healing`), so they need fork work. In
`~/git/rotator/board/models/Ag5400_simplified.step` (123 bodies written as voids of one pin),
three bodies are "not oriented and closed" and one has a wire that "is not simple". Part 2 of
`~/git/rotator/board/models/V8BR-1AX1-GH.stp` (the RJ45) has a wire that "is not simple". Check
with `part m <file>` in a `.gasm`; OpenCascade reads all of them.

## Streamlined blades read badly in `sim flow`

`examples/flow/caradonna/hover.gsim` (Caradonna-Tung rotor, measured CT 0.00459, 507 N) reads
309 N and about 3x the torque at 13 mm cells, and its wake opens into a cone instead of
contracting to 0.78 R (details in `docs/flow.md`, Hover rotor). Smagorinsky puts eddy
viscosity in the cells touching a wall, a skin friction near 0.04 against 0.003: replace it
with a model that vanishes at walls (WALE or Vreman, `src/flow/lattice.rs` shader) or add a
wall function, then recheck `examples/flow/cube.gsim`, the heatsink and
`flow::square_cylinder_re22000`. Scaling it by 0.1 at the wall already moved 20 mm cells from
110 N, 294 N·m to 173 N, 92 N·m; 0 diverges.

Tried WALE (Cw 0.5, central-difference gradients with wall ghost velocities, one extra
velocity buffer) plus a Smagorinsky floor of 0.05: `hover.gsim` at 20 mm read 129 N and
197 N·m, but at 13 mm read 310 N and 138 N·m, the same as Smagorinsky. WALE alone goes NaN
on the square cylinder at Cw below about 0.8, and with the floor it goes NaN at d = 24
(`square_cylinder_re22000`) while d = 12 stayed stable (drag 1.57, Strouhal 0.152, against 2.1
and 0.132). So the wall viscosity is not what limits 13 mm; look at resolution and the
staircase blade first.

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

`heat` on a part that spins bails in `src/sim/wind.rs` (`heating`); its swept cells get air
conductivity, so the rotor conducts like air. Carrying heat in it needs the thermal lattice's
per-cell conductivity to move with the rotor cells (`sims-flow` rotor passes).
