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
0.015. On the RTX PRO 6000 it does 8,300 million cell updates a second (128³,
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
Inlets ramp up over the first few thousand steps and sponge layers at the inlet and outlet pull
the flow toward the free stream, because an impulsive start in a low-viscosity box leaves
pressure waves sloshing between them for tens of thousands of steps (drag swung between +3.6
and -3.6 around a steady 1.65).

The sphere at Re 50 reads 1.654 against Clift's free-stream 1.574, 5% high at Re 20 and 100 as
well. Most of that is the box: `tools/foam/run.sh sphere` runs the same case in OpenFOAM v2412
(Docker, `snappyHexMesh` with four prism layers, steady laminar `simpleFoam`), with symmetry
planes 4D off the axis, which mirror the sphere at the same 8D spacing as the lattice's
periodic sides. It gives 1.6048 on 313,000 cells and 1.6060 on 1.72 million, so the lattice is
3.0% high in the same box. The likeliest cause is resolution: 20 cells across leaves the
boundary layer about three cells thick, and a finer sphere does not fit next to the vLLM
server. The test checks against OpenFOAM's 1.606 within 4%.
