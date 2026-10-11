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

### flow

Wind on a part or assembly, from the GPU lattice Boltzmann solver in `src/flow` (see
`docs/flow.md`). No mesh: the parts are voxelized onto a uniform grid of cubes `mesh` mm across.

```
sim flow
part head.gasm
wind 30 toward=1,0,0 about=0,0,-500
mesh 10
average 40
leave *screw* *nut*
expect drag<400
```

- `wind speed toward=x,y,z about=x,y,z` is in m/s; `toward` is the way the air moves and
  `about` is where moments are taken (default the origin).
- `mesh` is the cell size in mm. The default puts 32 cells across the widest side the wind
  meets.
- `average passes` is how long the forces are averaged, in the time the wind takes to cross
  that side (default 40, about five shedding cycles behind a bluff body).
- `air density= viscosity=` overrides air at 20 °C (1.204 kg/m³, 1.516e-5 m²/s).

Readings: `drag` along the wind, `side` across it and level, `lift` across it and up, `cda`
(drag over the dynamic pressure, m²), `moment.x`, `.y` and `.z` about the `about` point in N·m,
`pressure` and `suction` (the extremes of the mean surface pressure, Pa), and for an assembly
`part.drag`, `part.side` and `part.lift`. The field is the mean surface pressure. `--dry-run`
prints the grid, the GPU memory it needs and the number of steps.

The box runs 4 widths upstream, 5 downstream and 3 to each side. Nearer sides and inlet were
measured to read high: a 100 mm cube at 20 cells across reads Cd 1.30 and a stagnation pressure
of 1.16 q with the inlet 2 widths away and the sides 1.5, and 1.01 and 1.00 q in this box
(Hoerner gives 1.05). The flow is a large-eddy simulation that does not resolve boundary
layers, so drag on sharp-edged parts, where the separation points are fixed, is more certain
than on smooth rounded ones.

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
   it. Curved faces ask for an element per radian of turn, from the surface normals at each
   triangle's corners. Where any of these is below the target, the surface is meshed again
   finer there, grading back up at 0.3 of the distance, but never below a sixteenth of the
   target: threads and small gear teeth come out coarse rather than dragging the whole part
   down to their scale (a sixty-fourth made the forearm 940,000 nodes instead of 440,000); a
   smaller `mesh` resolves them. The second pass is seeded by
   splitting the first pass's triangles across their longest edge only: splitting into four
   made a thin strip face (a gear tooth flank) as fine across its width as along its length,
   and the ring gear robot parts took two minutes or more to mesh. They now take 6 to 13 s.
3. Conforming Delaunay. All surface points go into an incremental Bowyer-Watson
   tetrahedralization with exact predicates (`robust`). A surface triangle missing from it has
   its longest edge split (on the true surface), followed by Delaunay edge flips within each
   face, until every surface triangle is a tetrahedron face.
4. Refinement. Tetrahedra larger than the target or with a radius-edge ratio above 2 get a
   point at their circumcentre, never across the surface. A point that would land inside a
   surface triangle's diametral sphere splits that triangle instead, on the true surface.
   Slivers then get their circumcentre or a point near it, for at most a quarter more points:
   on features thinner than the elements (gear teeth on a coarse mesh) each insertion made
   new slivers, and an uncapped pass took a 14 mm wide gear from 8,000 points to 42,000.
5. Clean up. Flat tetrahedra lying against the surface are shed where that keeps the surface
   closed and manifold. Then interior points are moved to raise the worst element in their
   star, 2-3 and 3-2 flips replace poor elements wherever that raises the worst one, and the
   points are smoothed again.

The tests check every mesh closes, every face is shared by exactly two elements or lies on the
surface, the elements fill the surface exactly, and the volume is close to the solid's, on a
box, curved parts, thin walls and the example parts.

In an assembly each part is meshed at the `mesh` size or a sixth of its own diagonal,
whichever is smaller, so an M4 screw is not meshed at the 65 mm a whole rotator would get;
a part that fails to mesh, or meshes with an element flatter than 2 degrees, is tried again
at 0.6 of the size, up to five times, keeping the best. Flat elements wreck the solve rather
than the answer: the rotator mast tube at 50 mm elements had 0.0 degree slivers, and with them
the conjugate gradients stalled at a residual of 0.84 against a direct coarse solve and grew
without bound against the three-level one. A 2 mm wall boom needed 14 mm elements in its
assembly pose (5.1 degrees at 23 mm on its own). Two parts bond within a tenth of the smaller of
their two sizes. Elements of zero volume (under a millionth of the mean element volume) left
inverted after straightening are skipped; any other inverted element is an error that names
the part and the spot.

The whole rotator (44 parts, 2.8 million nodes, 8.5 million unknowns) solves in 256 s: 24 s
meshing, the bonding in parallel over pairs whose boxes come within the bond gap (it was 32 s
serial over all 1,900 pairs), 8 s assembly and 480 iterations.

Smoothing works through colour classes of points that share no element, in parallel.

Still to do: better sliver removal where every corner of an element lies on the surface (they
survive near tangent edges and tiny faces). Nodal stresses are averaged weighted by element
volume times the square of its mean ratio, so such a flat element cannot set the peak.

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
unknowns, the direct solver takes over. Stresses are taken at each element's nodes and averaged,
weighted by element volume and shape.

Measured on the 200 mm cantilever: at 284,000 unknowns the direct solver took 7 s and the
two-level solver 28 iterations and 1.8 s; at 796,000 unknowns direct took 58 s and 15 GB, the
two-level solver 28 iterations, 6 s and 1.4 GB. Bonded assemblies converge more slowly (the
penalty ties stiffen the system). Each tied node is smoothed together with the node it leans
on most, in clusters of up to eight solved exactly, instead of one node at a time: on two
stacked 20 mm blocks at 257,000 unknowns that took the solver from 213 iterations to 71, and at
1.2 million unknowns it takes 130 iterations and 50 s. The heat solver clusters the same way
(90 iterations to 39). The smoothing weight is set from the matrix: a dozen power iterations
estimate the largest eigenvalue of the smoothed operator and the weight is 1.2 over it, capped at
0.6. A fixed 0.6 overshot on the rotator azimuth frame (seven bonded parts, the clusters solved
exactly), whose solve stalled at a residual of 3e-4 after 3000 iterations; the estimated weight
there is 0.28 and it converges in 877.

When the corner-only coarse problem is over 20,000 unknowns its direct solve became the bulk of
each iteration (faer's sparse triangular solve runs on one thread whatever its parallelism
setting: 107 ms of 170 ms an iteration on the 560,000-unknown bolt joint). Above that size the
coarse problem gets a third level: two damped block-Jacobi sweeps, then a direct solve on
aggregates of neighbouring coarse nodes, each with the six rigid-body modes of its node cloud
as its unknowns, then two more sweeps. The bolt joint went from 98 iterations and 16.7 s to 154
and 10.2 s with the same answer.

The tie penalty is 10 times the stiffest material over the element spacing. It was 100, which
conditioned the system badly: the rotator azimuth frame took 795 iterations and 122 s at 100 and
takes 299 and 25 s at 10, with every probe within 0.7%. The cost is at the very edge of a bonded
patch, where the peak reads up to about 8% high (2.70 MPa on a stack carrying 2.5, against 2.62 at
100); probes half a millimetre away are within 0.4%. The frequency and buckling solvers factor problems up to 250,000
unknowns and above that run LOBPCG (block size the modes wanted plus three) preconditioned by
the same two-level scheme; a free part's six rigid-body modes are worked out from the node
positions and kept out of the search instead of shifted. The turret robot part at 1.6 million
unknowns gives its first three frequencies in 30 iterations and a minute; at 284,000 unknowns
the two ways take the same 10 s. The heat solver factors problems up to 150,000 unknowns
and runs the same two-level scheme (scalar) above that, up to 1.8 million: a 500,000 node bar
takes 13 iterations and 4.5 s where the direct solve took 20 s.

`thermal` (phase 4, built): the scalar version on the same quadratic mesh, conductivity from the
material, temperatures eliminated, film convection as a boundary mass term, solved with the
same sparse Cholesky. Checked against a wall (exact) and a convecting fin (0.5%).

`modal` (phase 5, built): consistent mass with a 14-point degree-5 rule, subspace iteration on
(K + sM)^-1 M with a tiny shift s so free parts factor too, Rayleigh-Ritz by Cholesky and
Jacobi rotations. Checked against a clamped and a free beam (under 1% on the first mode) and the
bending effective mass (61%).

Bonded assemblies (phase 6, built): each body is meshed on its own and the meshes are merged.
Every surface node of one body that lies within a tenth of the mesh size of another body's
surface is tied to the point under it by a penalty spring (10 times the stiffest material over
the element size), in both directions, which keeps interface stresses within a few percent
on a matching pair. Checked on a bonded stack in tension, a two-material wall in series and a
stacked block whose first mode matches the solid one.

`pin` holds the radial and axial directions of a round face's nodes; `torque` spreads a
moment as tangential traction proportional to the distance from the axis. Checked against a
shaft in torsion (stress and twist within 1%) and a link on two pins. Before solving, the six
rigid-body motions are projected through the supports, and a part they leave free is refused.

The viewer colours a `.gsim` by its result when the result is up to date, using a 1D colour
map texture on the boundary triangles. Hovering over the part shows the value under the cursor
(stress, temperature or mode shape), how far that point moved in a static run, and where it
sits on the undeformed part, interpolated across the boundary triangle it lands on. Loads, supports and
thermal boundaries are drawn too: an arrow for each load's resultant at the middle of its faces,
an outline round the faces each line acts on, hidden behind the part like any edge, a label with its total (`seat.side: 770.2 N (0, 767.9,
59.05)`, `bolts: fixed, holds 767.9 N`), and the whole list in a corner. `gcad render` draws
the arrows and lists the same lines under the plot, and `gcad sim` notes each applied load.

`buckling`: the static solution gives the stress at each Gauss point, which builds a geometric
stiffness; subspace iteration on K^-1 (-K_G) finds the largest positive eigenvalues, whose
inverses are the load factors. A clamped column matches Euler's load within 0.05%.

`contact a b`: the tie pairs between those parts become normal-only penalty springs along the
master face's normal, switched on where they press and off where they would pull, re-solving
until the set settles. Checked with two blocks heated (they press, as if bonded) and cooled
(they part, stress free).

Ties, bonded or contact, only join a node to a face it faces (normals within about 45 degrees of
opposite). Without that, a node on a hole wall 0.72 mm above a standoff's end face was tied to
it through solid material, and the rotator frame's top plate read 64,000 MPa there.

A tie holds u_s - sum w_k u_k to zero, which resists rigid rotation whenever the node is not
exactly on the master patch (x_s - sum w_k x_k = d, nonzero where two curved meshes meet), so
bonded joints leaked moment: a Ø15 rod bonded 20 mm into a bore and bent by 100 N at 222 mm
reported 11.1 N·m at its root instead of 22.2. Each master gets a rotation term, d x J^-1 r_k x,
with r_k the master nodes about their centre and J their inertia, so the tie measures the gap
less the turn of the master patch carried through d, which is exactly zero for any rigid motion.
Ties reaching more than a tenth of the gap along the master face are refused (a rod node 0.3 mm
below a bore mouth was tied to its edge). The rod now reads 22.200 N·m square or round, and the
bolt-joint cross-check agrees within 2% and solves in 98 iterations instead of 420.

A quadratic element whose mid-side nodes sit on a curved surface can be valid but nearly
inside out, which gives huge stresses at one node. Elements whose Jacobian varies by more than
20 times across them get straight edges. That took the same frame from a 5,900 MPa false peak
to 189 MPa at a real sharp corner, without moving the cross-checked numbers.

`friction=`: Coulomb friction on the same ties, solved as a sequence of Tresca problems. With
each tie's slip limit held at the friction coefficient times its pressing gap, the tangential
spring becomes a Huber function (quadratic while it sticks, linear once it slips), so the
problem has a convex energy; Newton with an energy line search solves it, then the limits are
updated from the new pressures until they stop changing. Two shortcuts both failed: dropping
the slipping springs and pushing back with a fixed force diverged, and a secant spring took
hundreds of solves near the limit. The tangent along the slip direction is 1e-8 of the penalty:
1e-4 or more stalled the Newton steps. A part that slides shows as displacement growing past a
thousand times the bonded answer. Two blocks pressed with 1000 N and friction 0.3 hold 297 N
sideways in about 40 solves and give way at 303 N.

## Cross-checks

`tools/xcheck/run.sh` meshes the same STEP exports with gmsh and solves them with scikit-fem
(quadratic elements on straight-sided tetrahedra, PARDISO for the solves), sharing no code with
gcad. It runs in about 2.5 minutes. Measured with gcad at the same or a finer mesh:

| case | gcad | reference |
|---|---|---|
| `bracket.gsim`, mesh 1.2: peak displacement | 0.003337 mm | 0.003326 mm |
| same, `top` probe | 0.001786 mm, 3.762 MPa | 0.001783 mm, 3.788 MPa |
| bracket fixed at `<Z`, first four frequencies | 43804, 43854, 69358, 99613 Hz | 43911, 43979, 69490, 99741 Hz |
| `heatsink.gsim`: hottest, coolest | 64.833, 63.935 °C | 64.832, 63.935 °C |
| rod in tube between plates, rod cooled 100 °C (penalty ties against one conforming mesh) | 142.2, 28.12 MPa | 143.3, 27.56 MPa |
| 10 x 10 x 200 steel column, buckling factors | 10.285, 10.285, 91.08 | 10.285, 10.285, 91.09 |
| 20 x 20 x 3 angle, 150 long, buckling factors (no closed form) | 36.48, 102.6, 211.7 | 36.48, 102.5, 211.4 |

Contact and friction are not cross-checked; they rest on the stack tests in `tests/simulation.rs`.
scipy's default direct solver needed minutes at 240,000 unknowns and pyamg 455 iterations, so
the scripts use PARDISO.

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
