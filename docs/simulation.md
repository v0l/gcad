# Simulation plan

gcad simulations follow agentee's shape: one spec file per simulation, a command that runs it
and writes a result beside the spec, a short list of readings, a coloured render, and a
warning when the result is older than what it was run on.

## Spec files

A simulation is a `.gsim` file, one operation per line like `.gcad` and `.gasm`, with `let`,
`if` and `include` working the same way.

```
sim static
part bracket.gcad
material aluminium
fix base.start
load top.end force=0,0,-200
mesh 1.5
expect stress<60 displacement<0.2
```

- `sim kind` comes first: `torque`, `static`, `thermal` or `modal`.
- `part file.gcad` or `part file.gasm` names what is simulated. Face selectors on the following
  lines are that file's own, and `part:faces` reaches a part of an assembly.
- `material` overrides the part's material for this simulation.
- `mesh size` sets the target element edge, `refine faces size=` makes it finer near faces.
- `expect` lines are checks: `gcad sim` fails when one does not hold, the way
  `interference none` fails an assembly.

```sh
gcad sim bracket.gsim             # run it, write bracket.result.json, print the readings
gcad sim bracket.gsim --dry-run   # build the mesh and print nodes, elements and memory
gcad render bracket.gsim x.png    # the part coloured by the result field, with a scale bar
gcad check bracket.gsim           # also warns when the result is stale
```

`bracket.result.json` holds the readings, the field at each mesh node, and a hash of the spec,
every part file it read and the gcad version; a mismatch makes the result stale. The viewer
opens a `.gsim` and colours the part by the field, with a slider for mode shapes and deflection
scale.

## Materials

`material` gains the properties the solvers need, beside density: Young's modulus, Poisson's
ratio, yield strength, thermal conductivity and expansion. Printed plastics (pla, petg, abs,
asa, nylon, tpu) get printed-part values, lower than the moulded ones, and the readings say so.
`material name E= nu= yield= k=` sets any of them for an unlisted material.

## Kinds

### torque

For assemblies with joints. Sweeps each joint over its range in `steps`, and at every pose
reports the torque (or force, for slides) each joint must hold against gravity and payloads,
from the mass and centroid of everything it carries. Couples carry torque through gear ratios,
so the motor's torque is reported as well as the joint's.

```
sim torque
part arm.gasm
gravity 0,0,-9.81
payload gripper 0.5 at=0,0,-40
sweep shoulder steps=24
expect shoulder.motor<0.4
```

No mesh. Runs in seconds.

### static

Linear elastic stress. Supports: `fix faces`, `slide faces` (held only along their normal),
`pin faces` (a round face held but free to turn). Loads: `load faces force=x,y,z` (spread over
the faces), `pressure faces p`, `gravity`, `torque faces n axis=`. Readings: peak von Mises
stress and where, peak displacement and where, minimum safety factor against yield, reaction
forces at each support.

### thermal

Steady conduction. Sources: `heat faces watts` and `heat body watts`; boundaries:
`convect faces h= ambient=`, `temperature faces t`. Readings: peak and mean temperature per
body, heat leaving through each boundary.

### modal

The lowest natural frequencies and mode shapes, from the same supports as `static`
(`modes 6`). Readings: each frequency and its effective mass along x, y and z.

## Meshing

Tetrahedra that follow the surface, from the start.

1. Tessellate every B-rep face to the target size with the kernel's triangulation, keeping the
   face index on every triangle so selectors map to element faces.
2. Fill the closed surface with tetrahedra, keeping the surface triangles as the boundary:
   a constrained Delaunay tetrahedralization written in Rust (`src/sim/tet/`). Bowyer-Watson
   insertion with exact orientation and insphere predicates (Shewchuk's adaptive arithmetic,
   from the `robust` crate), boundary recovery by inserting Steiner points on missing surface
   edges and faces, then Delaunay refinement (Shewchuk's terminating algorithm) to a
   radius-edge ratio of 2 and a minimum dihedral angle, followed by sliver removal. Its own
   tests: every surface triangle is a tetrahedron face, the tetrahedra fill exactly the solid's
   volume, no element is inverted, and the quality limits hold, on boxes, revolves, thin walls,
   holes, threads and the example parts.
3. Make the elements quadratic (10-node tetrahedra), placing mid-side nodes on the true B-rep
   surface for curved faces. Linear tetrahedra are too stiff in bending to trust.
4. Assemblies: parts that touch are meshed separately and bonded where their faces meet, by
   tying the nodes of one face to the other's elements.

`--dry-run` stops after meshing and prints the counts and the worst element.

## Solvers

On the GPU through wgpu, as agentee does, with a CPU fallback.

- `static` and `thermal`: preconditioned conjugate gradient with the matrix applied element by
  element in a compute shader, so the global matrix is never assembled. Jacobi preconditioning
  first; a smoothed-aggregation multigrid preconditioner if large parts converge slowly.
- `modal`: LOBPCG for the lowest modes on the GPU, using the same element kernels for the
  stiffness and mass.
- Small problems, and runs without a GPU, use `faer` sparse Cholesky on the CPU.

## Tests

Every kind is checked against closed forms, the way the volume tests check geometry:

| test | closed form |
|---|---|
| bar under tension | stress F/A, stretch FL/AE |
| cantilever tip load | tip deflection PL³/3EI, root stress PLc/I |
| plate with a hole under tension | peak stress about 3 times the nominal |
| thick cylinder under pressure | Lamé hoop stress |
| wall conducting heat | linear temperature, flux kA dT/L |
| fin with convection | tanh fin solution |
| cantilever modes | 1.875², 4.694², 7.855² times sqrt(EI/ρAL⁴)/2π |
| pendulum on a joint | torque m g r cos θ across the sweep |

Each also gets a mesh refinement check: halving the element size must move the reading toward
the closed form.

## Phases

1. Spec files, results, staleness, the field render, and the material properties. `torque`.
2. The tetrahedral mesher and `--dry-run`.
3. `static` on the CPU, checked against the closed forms, then on the GPU.
4. `thermal`.
5. `modal`.
6. Bonded assemblies.
