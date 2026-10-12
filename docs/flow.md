# Flow simulation plan

`sim flow` adds air and water flow around parts: forces and moments on each part, the
pressure over its surface (which a `static` simulation can take as its load), and the flow
field for the viewer. It runs on the GPU through wgpu, the same library the viewer draws with.

Decisions:

- **Lattice Boltzmann first, finite-volume RANS later.** Lattice Boltzmann is explicit and
  local, so every cell updates independently and the GPU stays busy. A finite-volume RANS
  solver (pressure projection, k-omega SST) comes after it as a second answer to check it with.
- **Large-eddy simulation for turbulence.** Lattice Boltzmann is unsteady by nature, so it
  resolves the large eddies and models the small ones (Smagorinsky subgrid viscosity from the
  cell's own non-equilibrium stress, so it needs no neighbours; WALE later if it is too
  dissipative near walls), and gives the
  mean force and the gusts. A wall model covers boundary layers thinner than a cell.
- **Benchmarks before the rotator.** Every step is checked on a case with a published answer,
  and each benchmark is also run in OpenFOAM so the two solvers are compared on known ground
  before they are compared on the rotator.
- **OpenFOAM in Docker.** The official `opencfd/openfoam` image; gcad writes each case
  (geometry as STL, `blockMesh` and `snappyHexMesh`, `simpleFoam` or `pimpleFoam` with
  k-omega SST) and `tools/foam/` runs it, the way `tools/xcheck/` runs scikit-fem.

## The device

Measured through wgpu 29 on the RTX PRO 6000 (Vulkan): 2 GiB per storage binding, 1 TiB per
buffer, 524,288 storage buffers per shader stage, f16, f64 and timestamp queries. Vulkan also
offers llvmpipe, a CPU device, which the tests use so they run without a GPU.

Each of the 19 directions of the D3Q19 lattice gets its own buffer (structure of arrays), so a
2 GiB binding limits a run to about 500 million cells, 38 GB with two copies. Populations are
stored as f32 shifted by their weight (f - w), which keeps the digits that f32 would otherwise
spend on the constant part.

## Phases

1. **Lattice core.** A headless wgpu device, D3Q19 streaming with two copies (pull), a BGK
   collision, then a regularized collision for stability, halfway bounce-back walls, a moving
   wall and a body force. Checks: Poiseuille flow between plates against the parabola, and the
   lid-driven cavity at Re 100 and 1000 against Ghia et al.'s centreline velocities.
2. **Parts in the flow.** Voxelize gcad solids into the lattice, interpolated bounce-back on
   their surfaces, a velocity inlet, a pressure outlet and slip side walls, and forces by
   momentum exchange. Checks: the Schaefer-Turek cylinder in a channel at Re 20 (drag 5.58,
   lift 0.0106) and Re 100 (Strouhal 0.30 in that channel), the sphere's drag against Re.
3. **Turbulence.** WALE subgrid viscosity and a wall model. Check: the square cylinder at
   Re 22,000 against Lyn et al. (drag about 2.1, Strouhal about 0.13).
4. **OpenFOAM comparisons.** The case writer and runner, and the benchmarks run in both.
5. **The `sim flow` spec.** Wind speed and direction, domain, cell size, run time and
   averaging window. Readings: drag, lift and moment per part and in total; the surface
   pressure as a result field; `wind flow.gsim` in a static simulation to load it with that
   pressure.
6. **The rotator.** The az/el head in wind against OpenFOAM, then its static sim loaded with
   the computed pressure instead of `q·CdA`.
7. **Finite-volume RANS on the GPU.** Steady k-omega SST on the same voxel grid, compared with
   the lattice solver and simpleFoam.

## Status

Phase 1 is built (`src/flow/`): D3Q19 with two copies and pull streaming, BGK or a
regularized collision (the non-equilibrium part projected onto its second-order stress), Guo
forcing, Smagorinsky subgrid viscosity, halfway bounce-back, moving walls. Poiseuille flow matches the parabola within 0.2% of
the peak; the Re 100 cavity on 128 x 128 matches Ghia et al. within 0.004 of the lid speed at
every tabulated point, and the Re 1000 cavity on 256 x 256 (regularized, tau 0.577) within
0.015. On the RTX PRO 6000 it does 8,200 million cell updates a second (128³,
f32, either collision), about 1.25 TB/s of memory traffic.

Traps: a moving-wall flag on the cavity's corner cells pumps mass in at one corner and out at
the other (the diagonal next to it gets the wall's momentum and its mirror does not), so the
flow never settles and drifts 7% to 18% high; the lid stops a cell short of each side wall.
Parts in the flow so far: a velocity inlet (equilibrium at the cell's own velocity, density
from the cell inside it) and a pressure outlet (equilibrium at density 1, velocity from the cell
inside it), applied by a second pass after each step; body numbers in the upper bits of solid
cells' flag words, and forces by momentum exchange summed per body; interpolated (Bouzidi)
bounce-back where a link's wall fraction is known, kept in a table that a fluid cell's flag
word points into. The Schaefer-Turek cylinder at Re 20, 40 cells across: staircase walls gave
drag 5.744 (2.9% high), interpolated walls 5.703, then 5.685 at 60 cells, so the rest was not
resolution: at a peak inlet speed of 0.075 the Mach number is 0.13, and halving it gave 5.602,
within 0.4% of 5.5795. At Re 100 (case 2D-2, same grid) the shedding gives peak drag 3.229
(published 3.22 to 3.24), Strouhal 0.302 (0.295 to 0.305) and peak lift 0.963, 3% under the
published 0.99 to 1.01. Lattice speeds stay at or under about 0.04 for force work.

The regularized collision kept only the second-order stress of the non-equilibrium part and so
dropped its first moment, which under Guo forcing is -F/2: each step added (1 - omega) F/2 of
momentum, and at tau 0.6 plates and a pipe carried exactly 2/3 of the right flow. The first
moment is now kept (-1.5 w c.F per direction). The plates test was at tau 1, where that term is
multiplied by zero, which is how it got through; it now runs at tau 0.6, and a round pipe with
interpolated walls matches Hagen-Poiseuille within 0.15%.

gcad solids go into the lattice with `flow::voxel::place`: cell centres inside the triangulated
surface (crossings along x per column, the rays nudged off the cell centres) become wall cells
of that body, and each link from a fluid cell into it gets its wall fraction from a ray cast
against the same triangles. A sphere 20 cells across voxelizes to 4,145 cells against 4,189.
Inlets ramp up over the first few thousand steps and a sponge layer at the outlet pulls the
flow toward the free stream, because an impulsive start in a low-viscosity box leaves pressure
waves sloshing for tens of thousands of steps (drag swung between +3.6 and -3.6 around a
steady 1.65). There is no sponge at the inlet: one there held the density at the free stream
while the body's upstream influence wanted it to rise, and so fed total pressure into the flow
(a cube's stagnation pressure read 1.16 q, and the sphere's drag 1.6% high).

The sphere at Re 50 read 1.654 against Clift's free-stream 1.574, 5% high at Re 20 and 100 as
well. Most of that is the box: `tools/foam/run.sh sphere` runs the same case in OpenFOAM v2412
(Docker, `snappyHexMesh` with four prism layers, steady laminar `simpleFoam`), with symmetry
planes 4D off the axis, which mirror the sphere at the same 8D spacing as the lattice's
periodic sides. It gives 1.6048 on 313,000 cells and 1.6060 on 1.72 million. With the inlet
sponge removed the lattice reads 1.6285 at 20 cells across (1.4% high) and, before that fix,
1.6242 at 40 cells against 1.654 at 20, so what remains is resolution. The test checks 20
cells against 1.606 within 2.5%; `sphere_drag_re50_fine` runs 40 cells (61 million, ten
minutes).

### Turbulence

At Re 22,000 the viscosity leaves tau at 0.5001, where the lattice hardly damps sound. The
square cylinder's sharp corners fed a tone with a 32-step period that carried 72% of the lift
signal (lift swung by 5 against a shedding amplitude near 1.5). `Settings::bulk` relaxes the
trace of the non-equilibrium stress more slowly than the rest, which adds bulk viscosity
(tau_bulk = tau + 4.5 bulk) and damps sound without touching nearly incompressible flow. At
`bulk 0.1` the tone is gone and 96% of the lift energy sits at the shedding frequency.

The flow also stays two-dimensional when the box and inflow are exactly uniform along the span,
which gave drag 2.43 and Strouhal 0.155; a fixed 2% random disturbance on the inlet velocity
lets it break up. With Smagorinsky 0.14, bulk 0.1, D = 24 in a 22D x 14D x 4D box (periodic
sides, 7% blockage as in Lyn's tunnel), drag is 2.20 against Lyn's 2.1, Strouhal 0.145 against
0.132 and rms lift 1.1. The LES entries of the Rodi workshop on this case spread over drag
1.66 to 2.77 and Strouhal 0.09 to 0.15.

### `sim flow`

Phase 5 is built (`src/sim/wind.rs`, documented in `docs/simulation.md`): `wind`, `air`,
`average` and `mesh` as the cell size; drag, side, lift, CdA, moments, per-part forces and the
mean surface pressure as the field, sampled on the part surfaces subdivided to the cell size.
The Schaefer-Turek pressure difference at Re 20 checks the lattice pressure: 2.9429 against
2.9375. A 100 mm cube at 10 m/s and 20 cells across reads Cd 1.01 (Hoerner 1.05) and a
stagnation pressure of 1.00 q once the inlet is 4 widths upstream and the sides 3 widths away;
2 and 1.5 read Cd 1.30 and 1.16 q. The default box is therefore large, (L + 9H) x 7H x 7H, and
the default cell puts 32 cells across H.

Assemblies broke three things. The force pass added each link's share of the reference
density (the `2w` of the shifted storage), which sums to zero over a closed surface but not
over one part pressed against another: per-part forces came out in multiples of 3.7 N, the
lattice's 232 kPa reference on a single link, and the mast clamp read 1,707 N of lift in a
60 N wind. Forces are now gauge. Walls thinner than a cell came out porous, so `place` now
casts rays along all three axes and marks the nearest cell between two close crossings.
Interpolated walls on an assembly drove some cells to 2.4 q (a box corner, the inside of a
boom clamp) and no rule tried (one-cell gaps, a second fluid node, cells only reached through
narrow passages, skipping cells from the thin-wall pass) removed it, so `sim flow` uses
staircase walls and the benchmarks keep the interpolated ones.

`examples/flow/cube.gsim` is the quick benchmark: a 100 mm cube in 10 m/s air, 4 mm cells,
7.8 million of them. It took 48 s and now takes 24 (Cd 1.08 then 1.07). The kernel itself
was already at the memory limit: 8.2 billion updates a second is about 1.25 TB/s against the
card's 1.8, and dropping the integer modulo from neighbour indexing, skipping the neighbour
flag reads in cells with no wall beside them and wider workgroups all stayed inside the
run-to-run spread (7.7 to 8.3 billion). Only the first is kept. The time went elsewhere:

- Readbacks. Every sampling tick read two slices and the link forces, and every frame a
  full-resolution velocity snapshot (125 MB here), each followed by a wait for the GPU, which
  sat idle while the CPU traced streamlines. Reads are now requested with the tick's steps and
  taken one tick later, so the CPU works while the GPU runs the next steps. 48 s to 39 s.
- Averaging. The fixed 40 passes became an early stop once the mean drag is known to 1%,
  borrowed from OpenFOAM's `residualControl`. The cube stops at 20. 39 s to 24 s.

Two other OpenFOAM habits did not carry over. Starting the whole box at the free stream (as
`internalField uniform` does) instead of ramping the inlet left a transient that was still
lifting the cube's drag 5% forty passes later, against a flat history after the ramp. And f16
storage, a mixed-precision build in OpenFOAM's sense, runs at 15.1 billion updates a second
but is too coarse for the laminar benchmarks, so it is the opt-in `precision half`. Values
are stored as `f - w` scaled by 2^15 into f16's normal range; the scale did not change the
errors, which come from the 11-bit mantissa (a body force of 1e-6 per step is below one step of
a population near 1e-3).

### Heat

`heat` lines in a `sim flow` add a second lattice for temperature: D3Q7, two-relaxation-time
(the antisymmetric rate sets the diffusivity, `tau- = 1/2 + 4 alpha`, and the symmetric one the
magic `Lambda = 1/4`, which keeps it stable at the cell Peclet numbers of air near 80). It runs
in the same kernel as the flow, after the collision, with that cell's velocity, so it costs
seven populations each way (56 B a cell) and no second read of the flow. Part cells conduct
with their material's conductivity, with the air's heat capacity, so only the steady state is
right but a 167 W/m K fin settles as fast as the air around it. Walls that belong to no part
(duct walls, sealed pockets) bounce the temperature back, so no heat crosses them; the inlet is
an anti-bounce-back Dirichlet at 0 (the air's own temperature), the outlet copies the cell
inside it. Heat goes into the part cells nearest the heated faces, by area.

Checks: conduction through two materials 1:4 in conductivity with a source in one matches the
piecewise quadratic within 0.3% of the peak; a channel 20 cells high between heated walls at
Re 50 reaches Nu 8.232 and 8.239 at two stations against 8.235 for uniform flux (the wall
temperature taken at the interface, weighting the two cells by conductivity). The heatsink in
`examples/flow/heatsink.gsim` (eight 1.5 mm fins 20 mm tall on a 40 x 50 mm base, shrouded,
2 m/s, 10 W) reads 1.93 K/W at 0.5 mm cells and 1.85 at 0.33, against 1.89 from Teertstra,
Yovanovich and Culham's composite model for shrouded plate fin heat sinks (which leaves out
the base between the fins). It runs in 36 s.

The thermal lattice uses the air's molecular diffusivity only. Adding Smagorinsky's eddy
viscosity over a turbulent Prandtl number of 0.85, as LES usually does, read the heatsink 1.22
K/W at 0.5 mm and 1.41 at 0.33: in a laminar channel the eddy viscosity is a stabiliser for
the flow (tau is 0.5018 there and it diverges without it), not turbulence, and as a heat flux it
is simply wrong. Wind on parts that is truly turbulent therefore carries heat only through the
eddies the grid resolves.

`sim flow` blows the wind along lattice z. With the inlet on an x face the edge pass touched
one cell every nx along each of 38 population buffers, and at 547 x 571 x 471 that cost about
150 ms a step (throughput fell from 9.1 to 0.9 billion updates a second); the z faces are
contiguous, and a z-flow edge pass costs nothing measurable. The sponge's axis is a setting
(`sponge_axis`), x for the benchmarks.

The rotator head (`~/git/rotator/sims/wind.gsim`, 120 km/h side wind, no antennas):

| | drag | booms | el box and lid | az box and lid | mast |
|---|---|---|---|---|---|
| lattice, 9 mm cells (26 M) | 79.9 N | 18.3, 17.7 | 16.0 | 14.7 | 8.7 |
| lattice, 5 mm cells (147 M) | 63.8 N | 12.2, 12.3 | 14.3 | 13.7 | 7.6 |
| OpenFOAM k-omega SST, 2.1 M cells | 39.9 N | | | | |
| OpenFOAM k-omega SST, 11.3 M cells | 34.9 N | | | | |

The booms are 25 mm tubes; on their own the lattice reads Cd 1.89, 1.36 and 1.03 at 2.8, 5 and
10 cells across, against about 0.98 from Hoerner, so the 5 mm run is still about 7 N high on
the booms. OpenFOAM's RANS reads the same lone tube at Cd near 0.2: steady RANS cannot hold the
laminar separation of a round section at Re 55,000 and gives it post-crisis drag. Neither
solver is right on this assembly, then; the lattice converges toward the measured cylinder
drag as its cells shrink and the RANS model does not, so the head takes about 55 to 60 N in
this wind, not 35.
