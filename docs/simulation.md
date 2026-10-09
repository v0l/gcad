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
every part file it read, the `--set` variables and the gcad version; a mismatch makes the
result stale. The viewer
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

Tetrahedra that follow the surface, written in Rust (`src/sim/tet/`). Built in phase 2:

1. The surface. Each B-rep face is triangulated in its own parameter space with `spade`
   (constrained Delaunay, refined to 25 degree angles), its parameters scaled by the surface's
   mean metric so triangles come out the right size in 3D. Face boundaries come from the
   kernel's edge polylines, split the same way on both sides, so neighbouring faces share
   their points exactly. Each triangle keeps its B-rep face index, so selectors map to
   element faces.
2. The size field. A first pass at the target size is ray cast along every triangle's normal
   to find wall thickness and gaps, and every B-rep edge is measured against the edges near
   it. Where either is below the target, the surface is meshed again finer there, grading
   back up at 0.3 of the distance.
3. Conforming Delaunay. All surface points go into an incremental Bowyer-Watson
   tetrahedralization with exact predicates (`robust`). A surface triangle missing from it has
   its longest edge split (on the true surface), followed by Delaunay edge flips within each
   face, until every surface triangle is a tetrahedron face.
4. Refinement. Tetrahedra larger than the target or with a radius-edge ratio above 2 get a
   point at their circumcentre, never across the surface. A point that would land inside a
   surface triangle's diametral sphere splits that triangle instead, on the true surface.
   Slivers then get their circumcentre or a point near it.
5. Clean up. Flat tetrahedra lying against the surface are shed where that keeps the surface
   closed and manifold. Then interior points are moved to raise the worst element in their
   star, 2-3 and 3-2 flips replace poor elements wherever that raises the worst one, and the
   points are smoothed again.

The tests check every mesh closes, every face is shared by exactly two elements or lies on the
surface, the elements fill the surface exactly, and the volume is close to the solid's, on a
box, curved parts, thin walls and the example parts.

Still to do: quadratic (10-node) elements with mid-side nodes on the true surface, which come
with the solver in phase 3; and better sliver removal where every corner of an element lies
on the surface (they survive near tangent edges and tiny faces).

## Solvers

`static` (phase 3, built): the B-rep mesh becomes 10-node tetrahedra, their mid-side nodes put
on the true surface wherever that keeps every element's Jacobian positive. Element stiffness
uses a 4-point rule; supports are projections per node (a fixed node keeps nothing, a sliding
node loses its normal direction), so they need no penalty. Problems up to 100,000
unknowns go to `faer`'s sparse Cholesky, single threaded (its threaded mode was slower here and
spent most of its time in the kernel). Larger ones use conjugate gradients preconditioned by a
two-level cycle: two damped block-Jacobi sweeps, a coarse correction on the linear (corner-only)
tetrahedra of the same mesh, built as the Galerkin product of the quadratic matrix and factored
directly, and two more sweeps. If that has not converged and the problem is under 600,000
unknowns, the direct solver takes over. Stresses are taken at each element's nodes and averaged.

Measured on the 200 mm cantilever: at 284,000 unknowns the direct solver took 7 s and the
two-level solver 28 iterations and 1.8 s; at 796,000 unknowns direct took 58 s and 15 GB, the
two-level solver 28 iterations, 6 s and 1.4 GB. Bonded assemblies converge more slowly (the
penalty ties stiffen the system): the two-body case example at 2 million unknowns took 771
iterations and two minutes. The frequency and heat solvers are still direct only, so they refuse
problems above 600,000 and 1.8 million unknowns.

`thermal` (phase 4, built): the scalar version on the same quadratic mesh, conductivity from the
material, temperatures eliminated, film convection as a boundary mass term, solved with the
same sparse Cholesky. Checked against a wall (exact) and a convecting fin (0.5%).

`modal` (phase 5, built): consistent mass with a 14-point degree-5 rule, subspace iteration on
(K + sM)^-1 M with a tiny shift s so free parts factor too, Rayleigh-Ritz by Cholesky and
Jacobi rotations. Checked against a clamped and a free beam (under 1% on the first mode) and the
bending effective mass (61%).

Bonded assemblies (phase 6, built): each body is meshed on its own and the meshes are merged.
Every surface node of one body that lies within a tenth of the mesh size of another body's
surface is tied to the point under it by a stiff penalty spring (scaled to the stiffest material
and the element size), in both directions, which keeps interface stresses within a few percent
on a matching pair. Checked on a bonded stack in tension, a two-material wall in series and a
stacked block whose first mode matches the solid one.

`pin` holds the radial and axial directions of a round face's nodes; `torque` spreads a
moment as tangential traction proportional to the distance from the axis. Checked against a
shaft in torsion (stress and twist within 1%) and a link on two pins. Before solving, the six
rigid-body motions are projected through the supports, and a part they leave free is refused.

The viewer colours a `.gsim` by its result when the result is up to date, using a 1D colour
map texture on the boundary triangles.

`buckling`: the static solution gives the stress at each Gauss point, which builds a geometric
stiffness; subspace iteration on K^-1 (-K_G) finds the largest positive eigenvalues, whose
inverses are the load factors. A clamped column matches Euler's load within 0.05%.

`contact a b`: the tie pairs between those parts become normal-only penalty springs along the
master face's normal, switched on where they press and off where they would pull, re-solving
until the set settles. Checked with two blocks heated (they press, as if bonded) and cooled
(they part, stress free).

Still to build: friction, an iterative path for `modal` and `thermal`,
and a better smoother for bonded assemblies.

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

1. Spec files, results, staleness, charts, the material properties, and `torque`. Done.
2. The tetrahedral mesher, `--dry-run` and mesh renders. Done.
3. `static` on the CPU, checked against the closed forms. Done; the GPU waits on a multigrid
   preconditioner.
4. `thermal`. Done.
5. `modal`. Done.
6. Bonded assemblies. Done.
