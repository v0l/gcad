# Flow simulation plan

`sim flow` adds air and water flow around parts: forces and moments on each part, the
pressure over its surface (which a `static` simulation can take as its load), and the flow
field for the viewer. It runs on the GPU through wgpu, the same library the viewer draws with.

Decisions:

- **Lattice Boltzmann first, finite-volume RANS later.** Lattice Boltzmann is explicit and
  local, so every cell updates independently and the GPU stays busy. A finite-volume RANS
  solver (pressure projection, k-omega SST) comes after it as a second answer to check it with.
- **Large-eddy simulation for turbulence.** Lattice Boltzmann is unsteady by nature, so it
  resolves the large eddies and models the small ones (WALE subgrid viscosity), and gives the
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

Phase 1 core is built (`src/flow/`): D3Q19 with two copies and pull streaming, BGK with Guo
forcing, halfway bounce-back, moving walls. Poiseuille flow matches the parabola within 0.2% of
the peak; the Re 100 cavity on 128 x 128 matches Ghia et al. within 0.004 of the lid speed at
every tabulated point. On the RTX PRO 6000 it does 8,200 million cell updates a second (128³,
f32), about 1.25 TB/s of memory traffic.

Traps: a moving-wall flag on the cavity's corner cells pumps mass in at one corner and out at
the other (the diagonal next to it gets the wall's momentum and its mirror does not), so the
flow never settles and drifts 7% to 18% high; the lid stops a cell short of each side wall.
The GPU on this machine is shared with a vLLM server holding 92 of its 96 GB, so runs are
capped at about 3 GB (some 20 million cells) while that is up.
