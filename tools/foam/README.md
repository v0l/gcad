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
