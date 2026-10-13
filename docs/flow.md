# Flow solver

`sim flow` (spec in `docs/simulation.md`) runs a lattice Boltzmann solver on the GPU through
wgpu (`src/flow/`, `src/sim/wind.rs`). It gives forces and moments per part, the mean surface
pressure (a `static` sim can load it with `wind`) and the flow for the viewer.

## Method

- D3Q19, two copies, pull streaming, one buffer per direction. A 2 GiB binding caps a run near
  500 million cells (38 GB). Populations are stored as `f - w`, f32, or f16 scaled by 2^15 with
  `precision half`.
- Regularized collision, keeping the first moment under Guo forcing (dropping it carried 2/3 of
  the flow at tau 0.6). Smagorinsky 0.14 from the cell's own stress. `bulk 0.1` adds bulk
  viscosity: at tau 0.5001 a square cylinder's corners rang at a 32-step period that carried
  72% of the lift.
- Velocity inlet ramped over a few thousand steps, a pressure outlet, a sponge before the
  outlet. An impulsive start sloshed for tens of thousands of steps; a sponge at the inlet fed
  total pressure in (stagnation 1.16 q). A 2% random disturbance on the inlet breaks spanwise
  uniformity (without it a square cylinder read drag 2.43).
- Lattice wind speed 0.04: at 0.075 (Mach 0.13) the Schaefer-Turek drag read 1.5% higher.
- The wind runs along lattice z. An x inlet's edge pass strides across 38 buffers and took
  throughput from 9.1 to 0.9 billion updates a second.
- Box (L + 9H) x 7H x 7H, the inlet 4H upstream, H the square root of the frontal area. A
  cube read Cd 1.30 and stagnation 1.16 q with the inlet 2H away and the sides 1.5H. 32 cells
  across H unless `mesh` says.
- Parts are voxelized by cell centre (`flow::voxel::place`), with rays along all three axes
  marking walls thinner than a cell. `sim flow` uses staircase walls: interpolated (Bouzidi)
  walls drove assembly cells to 2.4 q at box corners and inside clamps. The benchmarks keep
  interpolated walls.
- Forces by momentum exchange per body, as gauge forces. Absolute ones summed the 232 kPa
  reference over parts pressed together and read 1,707 N of lift in a 60 N wind.
- Averaging stops once the mean drag is known to 1% (at least 20 passes). Readbacks are
  requested one tick ahead so the GPU never waits on the CPU.
- Heat: a D3Q7 two-relaxation-time lattice in the same kernel (56 B a cell), with part cells
  conducting at their material's conductivity and the air's heat capacity. Molecular
  diffusivity only: adding Smagorinsky's eddy diffusivity read the heatsink 1.22 and 1.41 K/W
  instead of about 1.85.

## Checks

| case | gcad | reference |
|---|---|---|
| Poiseuille between plates, round pipe | within 0.2%, 0.15% | exact |
| lid-driven cavity, Re 100 and 1000 | within 0.004 and 0.015 of the lid speed | Ghia et al. |
| Schaefer-Turek cylinder, Re 20, 60 cells | drag 5.602, pressure drop 2.9429 | 5.5795, 2.9375 |
| Schaefer-Turek cylinder, Re 100 | drag 3.229, Strouhal 0.302 | 3.22-3.24, 0.295-0.305 |
| square cylinder, Re 22,000 | drag 2.20, Strouhal 0.145 | Lyn 2.1, 0.132 (LES spread 1.66-2.77) |
| sphere, Re 50, 20 / 40 cells | 1.6265 / 1.6150 | OpenFOAM, same box, 1.606 |
| cube, 10 m/s, 20 cells | Cd 1.01 | Hoerner 1.05 |
| channel between heated walls, Re 50 | Nu 8.232, 8.239 | 8.235 |
| `examples/flow/heatsink.gsim`, 0.5 / 0.33 / 0.25 mm | 1.925 / 1.854 / 1.845 K/W | OpenFOAM 1.914, Teertstra 1.887 |

## Against FluidX3D and OpenFOAM

RTX PRO 6000 (1.8 TB/s), 2026-10-13. FluidX3D `dbf0bb5` runs the same boxes, lattice speed
and steps through `tools/fluidx3d/run.sh CASE`, with its own wind tunnel (equilibrium
boundaries, SRT with Smagorinsky, impulsive start, staircase walls). OpenFOAM v2412 runs
`tools/foam/` on 40 ranks.

| 256³ kernel, million updates/s | f32 | f16 |
|---|---|---|
| gcad (`flow::throughput`) | 8,891 | 13,970 |
| FluidX3D | 8,913 | 17,607 |

| case | gcad | FluidX3D FP32 | OpenFOAM |
|---|---|---|---|
| 100 mm cube, 10 m/s, 4 mm (Hoerner 1.05) | 1.066, 23 s (f16 1.065, 18 s) | 1.092, 20 s (FP16 1.073, 11 s) | 1.085, 203 s, SST |
| sphere Re 50, 20 cells (same box 1.606) | 1.6265, 40 s | 1.6834, 33 s | 1.6048, 20 s, laminar |
| sphere Re 50, 40 cells | 1.6150, 834 s (x-axis inlet) | 1.6393, 543 s | 1.6045, 42 s |
| 25 mm tube, 33 m/s, 5 mm (Hoerner 0.98) | 1.376, 29 s | 0.871, 24 s | 0.332, 285 s, SST |
| same tube, 2.5 mm | 1.022, 349 s | 0.930, 357 s | |

At 5 mm the tube voxelizes to a 4 x 4 square, a 20 mm square bar, in both lattices. That shape
should read about 1.3 on 25 mm (gcad's square cylinder matches Lyn), so FluidX3D reads it low.
Placed 0.27 cells off that alignment, FluidX3D reads 0.828: at five cells across a round
member's drag depends on where it lands. Both are within 5% at ten cells. Steady RANS cannot
hold laminar separation on a round section at this Reynolds number and reads a third of it.

## The rotator head

`~/git/rotator/sims/wind.gsim`, 120 km/h side wind, no antennas:

| | drag | booms | el box | az box | mast |
|---|---|---|---|---|---|
| 9 mm cells (26 M) | 79.9 N | 18.3, 17.7 | 16.0 | 14.7 | 8.7 |
| 5 mm cells (147 M) | 63.8 N | 12.2, 12.3 | 14.3 | 13.7 | 7.6 |
| OpenFOAM SST, 11.3 M cells | 34.9 N | | | | |

The 25 mm booms are square bars at 5 mm (Cd 1.36 on their own, about 7 N high); the head takes
about 55 to 60 N. RANS reads the booms post-critical and is too low.

## Fans

A fan is a part on a `turn` joint, and the gsim spins it: `spin fan rpm=4000`. The rotor
moves through the lattice as solid cells, so the blades push the air, shed wakes and feel
their own torque, with no fan curve supplied.

- **Moving geometry** (`src/flow/rotor.rs`). The rotor is voxelized once in its own frame on a
  grid twice as fine as the lattice. Before each step a GPU pass over the cells its blades can
  reach turns the eight sub-cell centres of each cell back by the rotor's angle and samples
  that grid: a cell with at least four of them inside is a wall of the rotor, the rest air.
  Cells the blades leave are filled at equilibrium with the blade's own velocity and the mean
  density of the air beside them. The angles come from a uniform buffer with one slot per
  step of a batch, so they are exact in f64 however long the run.
- **Wall velocity per body.** Bounce-back on a moving wall adds `6 w c.u`, with `u = omega x
  r` at the link's midpoint, from a table of each body's angular velocity and axis point; the
  cavity's single lid speed stays as body 0. Momentum exchange takes the same term back off and
  adds Wen's Galilean correction, and the torque is the moment of the link forces about the
  axis (a link's moment about its cell centre is exact, since the force lies along the link).
- **Forces summed every step.** A rotor's force read at one step is mostly noise from the
  cells it has just covered and uncovered; a second force pass adds every step into a buffer,
  read and cleared once a tick. Reading the force after the rotor had moved but before the
  step that used its new cells read the test fan's torque 43% high, which is why the geometry
  moves at the start of a step, not the end.
- **Time step from the tip.** The tip speed sets the lattice speed when it is above the wind.
- **Open air.** With no wind every face of the box is a pressure boundary, behind a sponge
  half an H deep that pulls the air toward rest, and air coming in through a face pays for its
  speed out of its pressure (total pressure 0, as OpenFOAM's `totalPressure` inlet). With the
  sides periodic, as for wind, the fan pumped the whole box like an infinite row of fans with
  nothing to stop them, and the flow kept growing until it diverged. Plain pressure faces (the
  reference density, the velocity of the cell inside) made noise of their own: the outer three
  cells of the box read 5.5 m/s against a 6 m/s jet, the band 8 to 20 cells in climbed from
  0.07 to 2.2 m/s over the run, and closing the sides with walls stilled the sides but not the
  ends. With the sponge and the inflow pressure the edge reads 0.00 m/s and 30 to 60 cells in
  0.09 (0.43 before); the fan's torque and flow did not move, its thrust rose 5%.

Checks:

1. Taylor-Couette (`flow::taylor_couette_torque`): a cylinder 32 cells across turning inside
   a fixed one 64 cells across, axially periodic, against `T = 4 pi mu omega R1² R2² / (R2² -
   R1²)` with the radii taken from the cells' areas: 0.23% high. At 16 and 32 cells it was 1.7%.
2. A square rod 48 cells across turning inside a fixed round wall at Re 0.5, against the same
   rod held still inside the wall turning the other way
   (`flow::turning_square_matches_the_turning_frame`). In creeping flow the two are one flow
   seen from two frames, and the torques agree within 1.3% (2% for a rod 24 cells across), so
   the cells changing hands cost little.
3. The flat-bladed rotor of `examples/flow/fan.gcad` (80 mm, four 16 x 2 mm blades at 30
   degrees, 3000 rpm) alone in still air (`examples/flow/rotor.gsim`), against OpenFOAM's
   frozen rotor (`tools/foam/fan.sh`, MRF, k-omega SST, 1.57 million cells, mean of
   iterations 1000 to 3000, which swing about 8%):

| | torque | thrust | flow through the disc | time |
|---|---|---|---|---|
| lattice, 2 mm (6.9 M cells) | 0.865 mN·m | 0.0167 N | 0.0109 m³/s | 34 s |
| lattice, 1.4 mm (20 M) | 0.950 | 0.0266 | 0.0119 | 173 s |
| lattice, 1 mm (55 M) | 0.694 | 0.0279 | 0.0119 | 782 s |
| OpenFOAM MRF, 1.57 M cells | 0.906 | 0.0317 | 0.0116 | 27 min on 40 cores |

The wake does not agree. Axial speed in m/s along radial lines, the lattice from its two mean
slices at 1.4 mm and OpenFOAM averaged over eight angles (`system/sampleDict` in the case):

| z | r = 2 | 10 | 18 | 26 | 34 | 42 | 50 mm |
|---|---|---|---|---|---|---|---|
| +20, lattice | -1.02 | -1.12 | -1.23 | -1.20 | -1.06 | -0.77 | -0.50 |
| +20, OpenFOAM | -1.22 | -1.30 | -1.38 | -1.32 | -1.08 | -0.71 | -0.40 |
| -10, lattice | 0.19 | 0.19 | -0.76 | -3.31 | -4.17 | -0.88 | -0.52 |
| -10, OpenFOAM | 0.14 | -0.22 | -3.09 | -4.54 | -2.88 | 0.49 | 0.23 |
| -60, lattice | 0.82 | 0.79 | 0.73 | 0.54 | 0.23 | -0.22 | -0.46 |
| -60, OpenFOAM | 0.02 | -1.51 | -3.30 | -2.45 | -1.59 | -1.67 | -1.33 |
| -100, lattice | 0.30 | 0.29 | 0.28 | 0.25 | 0.22 | 0.18 | 0.14 |
| -100, OpenFOAM | -2.40 | -2.36 | -2.08 | -1.67 | -1.26 | -1.04 | -0.95 |

The air drawn in (z = +20) agrees within about 15%. Behind the rotor the lattice's jet is a
hollow cone that opens outward, with air flowing back up the axis at 0.8 m/s behind the hub;
OpenFOAM's closes onto the axis, 2.4 m/s there 100 mm down. The lattice's cone is the same at
1 mm (back flow 0.9 m/s on the axis at z = -60, the cone a few millimetres wider), so it is
not the grid. OpenFOAM's swirl number behind the rotor is 0.40 to 0.45, close to where
swirling jets break down into exactly such a cone; steady k-omega SST is known to damp that
breakdown and a frozen rotor carries no blade passing, so neither answer is settled. A
transient OpenFOAM run with a sliding (AMI) rotor and a scale-resolving model, or a measured
fan wake, would settle it.

The flow through the disc agrees within 5% at every size. The thrust climbs toward OpenFOAM's as the cells
shrink. The torque does not settle: at 30 degrees the blades meet the air near 18 degrees and
are stalled, and both solvers model that separated flow, not resolve it.

### Hover rotor (Caradonna and Tung)

`examples/flow/caradonna/hover.gsim` is the Caradonna-Tung rotor of NASA TM 81232 (1981): two
untwisted, untapered NACA 0012 blades, R = 1.143 m, aspect ratio 6 (chord 190.5 mm), 8 degrees
collective, 1250 rpm, measured CT = 0.00459 (507 N in air of 1.204 kg/m³). The report gives no
root cutout; the model's blades start at 0.09 R on a hub of 0.1 R, which adds under 1% of the
thrust. At 13 mm cells (73 million, 15 cells on the chord, 1.8 through the thickness, 40 minutes
on the GPU with 17 GB free) the lattice is far off:

| | thrust | CT | torque |
|---|---|---|---|
| measured | 507 N | 0.00459 | |
| lattice, 13 mm | 309 N | 0.00280 | 139 N·m (CQ 0.0011, about 3x what a figure of merit near 0.6 implies) |

and its wake opens into a cone, where a hovering rotor's contracts to about 0.78 R.

Two causes were found:

- **Eddy viscosity at the wall.** Smagorinsky's model gives the first fluid cell an eddy
  viscosity of about `(Cs dx)^2 U/dx`, a skin friction coefficient near `2 Cs^2 = 0.04`
  against 0.003 for a turbulent plate at this Reynolds number. Scaling it down in cells that
  touch a wall, at 20 mm cells: factor 1 read 110 N and 294 N·m, 0.3 read 140 N and 213 N·m,
  0.1 read 173 N and 92 N·m; 0 diverged at 13 mm, the wall cells then sitting at tau 0.5000003.
  A wall-adapting model (WALE, or Vreman) or a wall function is needed, not a factor.
- **Resolution.** On a uniform grid a 2.3 m rotor in a box 7 H across leaves 15 cells on the
  chord and under 2 through the blade; thrust went from 110 N at 20 mm to 309 N at 13 mm and
  has not settled. Rotor codes refine the grid round the blades; this lattice is uniform.

Refilling uncovered cells with the surrounding air's velocity instead of the blade's made it
worse (181 N, 179 N·m at 13 mm), so the blade's own velocity stays.

## Left to do

- Local grid refinement. 5 mm over the rotator is near the card's memory, and round members
  need 10 cells across (2.5 mm for the booms); 1 mm everywhere would need about 2.8 TB.
- A wall model, and WALE if Smagorinsky proves too dissipative near walls.
- Steady finite-volume RANS on the same grid, as a second answer.
- Raise f16 throughput from 79% of FluidX3D's, and check `precision half` on the rotator.
