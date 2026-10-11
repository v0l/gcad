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
