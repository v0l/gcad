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
node loses its normal direction), so they need no penalty. Problems up to 600,000 unknowns go
to `faer`'s sparse Cholesky, single threaded (its threaded mode was slower here and spent most
of its time in the kernel); larger ones use conjugate gradients with a 3x3 block Jacobi
preconditioner. Stresses are taken at each element's nodes and averaged.

Measured: the 200 mm cantilever at `mesh 2` (283,000 unknowns) solves in 7 s; at `mesh 4` in
under a second. Conjugate gradients needed 7,000 iterations and 26 s on the same cantilever, so
the GPU port planned here is only worth it together with a better preconditioner (multigrid).

Still to build: `pin`, `torque` loads, several bodies, `thermal` (conduction, the same mesh and
a scalar version of the same assembly) and `modal` (LOBPCG on the same stiffness plus a mass
matrix).

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
4. `thermal`.
5. `modal`.
6. Bonded assemblies.
