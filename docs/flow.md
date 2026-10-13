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

## Left to do

- Local grid refinement. 5 mm over the rotator is near the card's memory, and round members
  need 10 cells across (2.5 mm for the booms); 1 mm everywhere would need about 2.8 TB.
- A wall model, and WALE if Smagorinsky proves too dissipative near walls.
- Steady finite-volume RANS on the same grid, as a second answer.
- Raise f16 throughput from 79% of FluidX3D's, and check `precision half` on the rotator.
