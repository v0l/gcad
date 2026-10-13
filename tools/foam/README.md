# OpenFOAM comparisons

`run.sh CASE` copies `CASE/` to a work folder, fills in the Reynolds number, mesh levels and
iteration count from the environment, and runs blockMesh, snappyHexMesh, checkMesh and
simpleFoam in the `opencfd/openfoam-default:2412` image, then prints the last line of the
force coefficients. The part is an STL from `gcad export part.gcad part.stl`.

    gcad export sphere.gcad sphere.stl
    STL=sphere.stl REYNOLDS=50 ./run.sh sphere

| variable | default | meaning |
|---|---|---|
| `REYNOLDS` | 50 | with D = 1 and U = 1, nu = 1 / Re |
| `LEVEL` | 4 | refinement level at the part's surface (base cells are D/4) |
| `NEAR`, `WAKE` | 3, 2 | levels in a sphere of radius D and a box behind the part |
| `LAYERS` | true | four prism layers on the part |
| `ITERATIONS` | 2000 | simpleFoam stops earlier once p is under 1e-6 and U under 1e-7 |
| `PROCS` | 16 | MPI ranks |

`wind.sh WORK` runs a `sim flow` case in OpenFOAM: steady k-omega SST with wall functions,
symmetry sides, the box sized as the lattice sizes it (4 H upstream, 5 H downstream, 3 H to
each side, H the square root of the frontal area). The parts are turned so the wind blows along
+x, so the printed force is (drag, side, lift) in N.

    gcad export wind.gsim parts.stl
    gcad sim wind.gsim --dry-run          # prints H, "its square root of ... mm"
    STL=parts.stl TOWARD=0,1,0 SPEED=33.3 ACROSS=286.5 ./wind.sh /tmp/foam/head

| variable | default | meaning |
|---|---|---|
| `TOWARD`, `SPEED` | 1,0,0 and 10 | the `wind` line |
| `ACROSS` | none | H in mm |
| `CELLS` | 8 | background cells across H |
| `LOW`, `HIGH` | 3, 4 | refinement levels at the surface (each halves the cell) |
| `NEAR` | 2 | level in a box half an H around the parts |
| `ITERATIONS`, `PROCS` | 1500, 40 | |

The 100 mm cube at 10 m/s on 555,000 cells reads 0.656 N (Cd 1.09); the lattice reads Cd 1.01
and Hoerner gives 1.05.

`heatsink.sh WORK` runs `examples/flow/heatsink.gsim` as a conjugate heat transfer case:
`chtMultiRegionSimpleFoam`, laminar, the same 40 x 25 mm duct, 123 mm upstream and 154 mm
downstream as the lattice box, a hex mesh of `CELL` mm split into an air and an aluminium
region (k 167 W/m K), the heat as a flux on the base's underside. It prints the mean
temperature of that face and the resistance from it to the inlet air.

    CELL=0.5 ./heatsink.sh /tmp/foam/heatsink

| variable | default | meaning |
|---|---|---|
| `CELL` | 0.5 | cell size in mm (fins and gaps must be whole cells) |
| `SPEED`, `WATTS` | 2, 10 | air entering the duct in m/s, heat into the base |
| `ITERATIONS`, `PROCS` | 3000, 40 | |

`fan.sh WORK` runs a rotor in still air as a frozen rotor: steady k-omega SST with an MRF zone
in a cylinder just round the blades, every face of the box open (total pressure 0), the box
as the lattice sizes it (4.5 H along the axis each way, 3 H to each side). The rotor is an STL
in mm with its axis along z through the origin. It prints the force on the rotor (N), the
moment about the origin (N·m) and the flow through the disc at z = 0 (m³/s).

    gcad export examples/flow/fan.gcad rotor.stl
    STL=rotor.stl RPM=3000 ACROSS=46.1684 ./fan.sh /tmp/foam/fan

| variable | default | meaning |
|---|---|---|
| `RPM` | 3000 | turning about +z |
| `ACROSS` | none | H in mm, from `gcad sim rotor.gsim --dry-run` |
| `CELLS` | 8 | background cells across H |
| `LOW`, `HIGH` | 4, 5 | refinement levels at the blades |
| `NEAR`, `WAKE` | 3, 2 | levels in the rotating zone and in a cylinder 2 H past it each way |
| `ITERATIONS`, `PROCS` | 3000, 40 | |
