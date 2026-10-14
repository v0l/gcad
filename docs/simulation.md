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
gcad sim bracket.gsim -q          # no progress lines
gcad render bracket.gsim x.png    # the part coloured by the result field, with a scale bar
gcad check bracket.gsim           # also warns when the result is stale
```

While it runs, `gcad sim` reports each stage on stderr: building the model and its part files,
meshing (each part that takes over 2 s gets its own line, with its tries and refinement), bonding,
assembly, the solver's iterations and residual, the frequency solver's residual, the flow's
settling and averaging steps with the running drag, the torque poses, and writing the result.
Each line has the count done, a percentage and the time left where it can be told. On a
terminal one line updates in place; piped, a stage prints when it ends and every 10 s while it
runs. The viewer's simulation card shows the same stages as bars while its run button is busy.

`bracket.result.json` holds the readings, the field at each mesh node, and a hash of the spec,
every part file it read, the `--set` variables and the gcad version; a mismatch makes the
result stale. The viewer
opens a `.gsim` and colours the part by the field, with a slider for mode shapes and deflection
scale. Its simulation card lists the readings, the `expect` checks, the notes and each sweep as a
trace, and has a run button that solves the file in the background. A stale result is still
drawn, with a warning saying what changed. Before there is a result it draws the setup: the mesh
with its loads, supports and thermal boundaries for the meshed kinds, and for `flow` the parts
inside the duct or the air box, the wind arrow and the heated faces.

## Remote runs

`gcad sim x.gsim --remote` solves on a runner of the job server at https://sims.gcad.sh, paid
from a prepaid balance per second of the runner's time. Each runner sets its own price an hour;
the site and `gcad remote runners` list them with their GPU, measured FP32 TFLOPS and memory
bandwidth (`gcad gpu-bench`, hidden, measures both), video memory and CPU.

```sh
gcad remote signup                  # make an account, keep its token in ~/.config/gcad/remote.toml
gcad remote account                 # its id, balance and price; top up at the shop with the id
gcad sim wind.gsim --remote         # build here, solve there, write wind.result.json here
gcad sim wind.gsim --remote --max-minutes 20
gcad sim wind.gsim --remote --max-price 2.50   # only runners asking at most €2.50 an hour
gcad sim wind.gsim --remote --max-price 0      # only free runners; no balance needed
gcad remote runners                 # the runners, their hardware and price
gcad remote jobs                    # recent runs, their time and cost
gcad remote cancel ID               # stop a run; the time it used is charged
gcad remote login TOKEN             # use the account on another machine
```

The model is built locally and the job holds what the solvers read: the spec with its faces
already selected, every body as compressed B-rep, the materials, and the `stamp` of the local
files. Project files never leave the machine. A `wind flow.gsim` load goes as the flow's
surface pressure samples, which needs a fresh local flow result, and a `warm heat.gsim`
goes as that thermal simulation's own packed job, solved on the worker first. `torque` runs
in seconds and stays local. Jobs are CBOR, gzipped, and only a worker on the same gcad
build takes them (`gcad sim-job JOB OUT`, hidden, is what it runs).

A run goes to the first free runner on the same gcad build (`gcad --version`: the version and
the last commit that changed the code) asking at most `--max-price` (by default the dearest
runner online, so any of them). A runner can ask nothing; a run capped at 0 needs no balance. It holds `--max-minutes` (60 by default) at
that price from the balance while it runs, charges the seconds it used at the price of the
runner that took it, and gives back the rest. The runner stops it at the limit.
Rerunning the same command with unchanged files picks up the queued or running job instead
of sending another, and a finished one when the result here is missing or stale, so a client
that stopped while it waited loses nothing. `GCAD_SIMS_SERVER` and `GCAD_SIMS_TOKEN` override
the saved server and token.

## Materials

`material` gains the properties the solvers need, beside density: Young's modulus, Poisson's
ratio, yield strength, thermal conductivity and expansion. Printed plastics (pla, petg, abs,
asa, nylon, tpu) get printed-part values, lower than the moulded ones, and the readings say so.
`material name E= nu= yield= k=` sets any of them for an unlisted material. Material libraries
(`materials.toml`, MatML, FreeCAD cards; see Material libraries in `docs/format.md`) carry
properties along three axes as well, for printed and fibre parts; the solvers are isotropic and
take the weakest direction, with a note.

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
`pin faces` (a round face held but free to turn), `bearing faces axial` (a band held sideways at one ring, free to turn and tilt). Loads: `load faces force=x,y,z` (spread over
the faces), `pressure faces p`, `gravity`, `torque faces n axis=`. Readings: peak von Mises
stress and where (more than one element size of the part from supports and bonds, with the peak inside that band
as `support_stress`), peak displacement and where, minimum safety factor against yield, reaction
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

- `wind speed toward=x,y,z about=x,y,z` is in m/s (`toward=x` or `-y` work too); `toward` is the way the air moves and
  `about` is where moments are taken (default the origin).
- `mesh` is the cell size in mm. The default puts 32 cells across H, the square root of the
  frontal area (the parts' shadow on a plane facing the wind).
- `average passes` is how long the forces are averaged, in the time the wind takes to travel
  H. Without it the run averages until the mean drag is known to 1%: at least 20 passes, then
  after each batch of 4 it stops once the standard error of the batch means is 1% of the mean
  and the two halves of the run agree within 2%, and never past 40 (about five shedding
  cycles behind a bluff body). A note gives the error and where it stopped. With `average`
  the run takes exactly that many passes.
- `precision half` stores the populations as 16-bit floats (the arithmetic stays 32-bit):
  about 1.8 times the cell updates a second, for a preview. The cube in `examples/flow` moves
  about 1% in drag, but the laminar benchmarks drift (Poiseuille 2% low, the Re 1000 cavity 3%,
  the Schaefer-Turek peak lift 0.74 against 0.99), so results carry a note. `precision single`
  is the default.
- `air density= viscosity= temperature=` overrides air at 20 °C (1.204 kg/m³, 1.516e-5 m²/s);
  `temperature` is the air coming in, 20 °C unless given.
- `duct width height` closes the box into a duct of that cross-section in mm, centred on the
  parts, with walls that neither slip nor pass heat. The wind is the speed of the air entering
  it, as a fan in a duct would push it. Without it the parts stand in open air.
- `label: heat faces watts` heats the faces. The air carries the heat away: the parts conduct
  it with their materials' conductivity (`k`), so every part needs a material, and the air
  takes it on with the flow. Readings: `label` is the mean temperature of the heated faces,
  `temperature` the hottest part cell, `part.temperature` per part of an assembly, and
  `resistance` the heated faces' rise over the inlet air per watt (K/W). The field becomes the
  mean surface temperature, and a note says how much of the heat the air carries out of the box.
  Averaging also waits for the heated faces' temperature to be known to 1%. Only the steady
  state is meaningful: the parts are given the air's heat capacity so they warm in step with
  the flow.

- `spin joint rpm=` turns a part of an assembly on its `turn` joint (negative rpm turns it
  the other way), with everything the joint carries: a fan, a propeller or an impeller
  (`examples/flow/fan.gsim`). The blades move through the lattice and push the air
  themselves, so no fan curve is given. With no `wind` the box lies along the joint axis and
  every face of it is open air (a sponge layer at each face keeps the far air still), so the fan
  draws its own flow; with a `duct` the fan meets the
  duct's resistance. The time step comes from the tip speed when that is above the wind.
  Readings: `joint.torque` (N·m it takes to keep it turning, about the axis), `joint.power`
  (W at the shaft), `joint.thrust` (N, the air's push on it along the joint axis) and
  `joint.flow` (m³/s through the disc its blades sweep, along the joint axis, when that axis
  lies along the box), and with a duct `flow` through its far end. Forces on a spinning part
  are summed every step, and averaging waits for its torque and flow to be known to 1% (at most
  160 passes). A part that spins cannot carry `heat` yet.

Readings: `drag` along the wind, `side` across it and level, `lift` across it and up, `cda`
(drag over the dynamic pressure, m²), `moment.x`, `.y` and `.z` about the `about` point in N·m,
`pressure` and `suction` (the extremes of the mean surface pressure, Pa), and for an assembly
`part.drag`, `part.side` and `part.lift`. The field is the mean surface pressure,
sampled about a cell apart on a surface tessellated to a fifth of a cell. `--dry-run`
prints the grid, the GPU memory it needs and the number of steps. `gcad render wind.gsim flow.png
--flow` draws the air speed on an upright and a level slice along the wind through the
centre of the frontal area, the mean with streamlines and one moment of the wake, and
`--lines` draws the parts in grey with the mean streamlines round them in four 3D views,
coloured by speed (at most 300 of them). A spinning part with no wind seeds its lines on
rings across the disc its blades sweep and traces them both ways, so they show the air drawn
in as well as the jet. The viewer
draws streamlines of the mean flow in 3D, coloured by speed: the velocity is averaged over the
whole box on the GPU (on cells up to a few lattice cells across, at most 16 million of them),
lines are seeded on a grid a little upstream across the frontal area and traced through it,
and the ones the parts slow below 85% of the wind are kept. Particles run along them at the
local speed of the mean flow, so the air is seen to race past edges and stall in the wake.
While averaging, the run also keeps up to 240 frames of the two slices, the speed in a byte
a cell, and at each frame traces the same seeds through the velocity of that moment, so the
lines themselves move with the wake. Both go in `x.frames.bin` beside the result (79 MB for
the rotator at 9 mm, points in 16 bits across the lattice box). The seeds are picked once,
from a mean taken over the last third of the settling time, so a line does not come and go
between frames. A line stops where it closes a loop (it re-enters a cell it left a few
points earlier), so a line caught in an eddy is drawn once round the eddy rather than circling
until the step limit. With moving lines, the particles ride the lines of the frame shown. Each line carries two scores for
the viewer to rank by: turbulence, the largest root-mean-square swing of the velocity about its
mean along the line, as a share of the wind (sampled at every frame), and pressure, the
strongest mean gauge pressure it passes through, signed, from the mean density. "Rank lines
by" shows the most or least turbulent, or highest or lowest pressure, share of the lines, and
the particles follow the same lines. The rotator at 9 mm spans 1% to 50% turbulence and -616
to +650 Pa, against a dynamic pressure of 668 Pa. The viewer's
flow display replaces the parts list for a flow result: surface pressure, streamlines (mean or
moving) and particles each with an opacity, each slice off, mean, one moment or moving (the frames played
in a loop, with the seconds of air shown), and playback that holds or plays the frame (with back and next for single frames) while the particles keep moving, a speed and a scrubber for time.
Particles and moving slices are drawn over the scene, so they show through parts; hovering a
mean or moment slice reads the mean speed and the speed at that moment.

The box runs 4 H upstream, 5 H downstream and 3 H to each side of the parts, which keeps the
blockage near 2% whatever their shape (a long thin part does not inflate the box). Nearer
sides and inlet were measured to read high: a 100 mm cube at 20 cells across reads Cd 1.30
and a stagnation pressure of 1.16 q with the inlet 2 widths away and the sides 1.5, and 1.01
and 1.00 q in this box (Hoerner gives 1.05). The grid is offset half a cell so that faces on
round numbers fall between cell centres.

Parts are voxelized with plain (staircase) walls. A wall thinner than a cell still blocks the
flow: rays along all three axes mark the nearest cell wherever two surface crossings have no
cell centre between them, so enclosure walls stay airtight. Where two fluid cells are joined
by a diagonal link and both cells beside that link are wall, one of them becomes wall, or air
streams diagonally through a staircase wall one cell thick (the rotator's boxes leaked at the
lid's rounded top edge). Air counts as outside only where the inlet reaches it
through passages at least three cells wide, with the layer of cells beside them; the rest is
taken out of the flow and noted. An opening narrower than about three cells (a cable gland's
bore once the cable is left out, a slit at a lid) therefore closes its enclosure, and the
rotator's boxes, which leaked through the Ethernet gland at 5 mm cells, seal at 9 and at 5. A part with no cells of its own (inside another, or thinner
than a cell along every axis) is noted and reads no force. Forces are gauge forces, so parts
pressed against each other do not feel the lattice's reference pressure on one side only.

What it gets wrong: a round member a few cells across is a square bar to the lattice, and
reads close to a square bar's drag. A 25 mm tube 500 mm long across a 33 m/s wind reads Cd
1.89 at 2.8 cells across, 1.36 at 5 and 1.03 at 10 (Hoerner gives about 0.98 for a free-ended
cylinder of that length). Members need about 10 cells across, and cells that fine over a
whole assembly may not fit on the GPU. The flow is a large-eddy simulation that does not resolve boundary layers, so
drag on sharp-edged parts, where the separation points are fixed, is more certain than on
smooth rounded ones. Pockets of air in holes left by parts that were left out keep whatever
pressure they settle to and can set the `pressure` and `suction` readings.

`wind flow.gsim` in a static (or buckling) simulation loads its parts with the mean surface
pressure from that flow result: each boundary face of the mesh takes the pressure of the
nearest surface sample within two cells, and faces further than that from any sample take
none. The flow must have a fresh result. Wall shear is left out: on the cube it is 3% of the
drag and, acting on the top face, 10% of the moment about the base.

### magnetic

The magnetic field of a motor, an actuator or a coupling, solved on a 2D slice through it
the way FEMM does, times the stack length. The plan, the method and the checks are in
`docs/magnetic.md`.

```
sim magnetic
part motor.gasm
turn spin sweep=6 steps=12
iron stator rotor.main material=m19
magnet rotor.north Br=1.2 toward=radial
magnet rotor.south Br=1.2 toward=-radial
a: coil c0 c7 turns=40 current=10
a: coil c1 c6 turns=-40 current=10
mesh 0.5
```

The slice is square to the `turn` joint's axis. Parts named in `magnet`, `iron` and `coil`
lines are cut by it, everything else counts as air, and the parts the joint carries are the
rotor. There must be an airgap all the way round between the rotor's parts and the rest:
the solver splits it into rings and turns the rotor in whole steps of its mesh, so the mesh is
never rebuilt during a sweep. `mesh` is the element size in mm, half the airgap unless given.

Readings: `torque` on the rotor about the joint axis (N·m, the mean over a sweep),
`torque.max`, `torque.min`, and over a sweep `cogging` (peak to peak, when no coil carries
current) or `ripple`; `airgap`, the peak flux density in the gap (T); `part.b`, the peak in each
iron part; and for each coil label `label.flux`, its flux linkage at the first pose (Wb), and
over a sweep `label.emf`, the peak back-EMF at 1000 rpm (V) from the change of flux linkage.
The sweep is drawn as torque and flux linkage against angle, and the field is the flux density
on the slice at the first pose.

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
   closed and manifold and turns no new surface face more than 25 degrees from the old ones:
   shedding across a bend folded the surface over itself and left slivers nothing could fix.
   Then interior points are moved to raise the worst element in their star, 2-3 and 3-2 flips
   replace poor elements wherever that raises the worst one, points on the surface slide along
   their face (or along the edge where two faces meet) to raise the worst element around them,
   and the interior points are smoothed again.

The tests check every mesh closes, every face is shared by exactly two elements or lies on the
surface, the elements fill the surface exactly, and the volume is close to the solid's, on a
box, curved parts, thin walls and the example parts.

In an assembly each part is meshed at the `mesh` size or a sixth of its own diagonal,
whichever is smaller, so an M4 screw is not meshed at the 65 mm a whole rotator would get;
a part that fails to mesh, or meshes with an element flatter than half a degree, is tried again
at 0.6 of the size, up to five times, keeping the best. Flat elements wreck the solve rather
than the answer: the rotator mast tube at 50 mm elements had 0.0 degree slivers, and with them
the conjugate gradients stalled at a residual of 0.84 against a direct coarse solve and grew
without bound against the three-level one. A 2 mm wall boom needed 14 mm elements in its
assembly pose (5.1 degrees at 23 mm on its own). Two parts bond within a tenth of the smaller of
their two sizes. Elements of zero volume (under a millionth of the mean element volume) left
inverted after straightening are skipped; any other inverted element is an error that names
the part and the spot. Elements
between half a degree and 2 degrees do no harm, and the line used to sit at 2: the rotator's
electronics box at `mesh 3` (worst 1.0 degree) solved in 180 iterations and read within 1% of
a 1.8 mm mesh, but no retry reached 2 degrees, so it was meshed at 0.4 mm with 15.7 million
nodes instead of 78 thousand. The whole rotator reads within 0.3% either way, with 7% fewer
nodes at half a degree.

The whole rotator (2.8 million nodes, 8.5 million unknowns) solved in 256 s on 2026-10-11:
24 s meshing, the bonding in parallel over pairs whose boxes come within the bond gap (it was
32 s serial over all 1,900 pairs), 8 s assembly and 480 iterations. On 2026-10-13 its
`sims/rotator.gsim` meshes to 4.5 million nodes and takes 1,372 iterations and 22 minutes.

Smoothing works through colour classes of points that share no element, in parallel.

Across the five example parts at twelve sizes from 1.2 to 4 mm, the worst element went from
0.27 to 5.2 degrees once surface points could slide and shedding stopped folding the surface.
Nodal stresses are averaged weighted by element volume times the square of its mean ratio, so
a flat element cannot set the peak.

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
on a matching pair. A clearance under that distance is bridged: pins in their holes, coils in
their slots and screws a few hundredths off their countersinks carry load that way, and so
would a cover 0.5 mm over a gear at 6 mm elements. Geometry cannot tell those apart, so
`apart cover wheel` keeps a named pair from bonding. Checked on a bonded stack in tension, a two-material wall in series and a
stacked block whose first mode matches the solid one.

`pin` holds the radial and axial directions of a round face's nodes, so the face cannot tilt:
on a bearing band 2 mm wide it reacted 14 and 29 N·m of moment. `bearing` holds only the radial
direction, on the one ring of nodes at the band's edge nearer the middle of the body, so the
section tilts and turns freely about it; `axial` adds the axial direction at one node of that
ring. A shaft on two bearings under a central load reads half the load at each, no moment, and
the beam-theory deflection within 1%. `torque` spreads a
moment as tangential traction proportional to the distance from the axis. Checked against a
shaft in torsion (stress and twist within 1%) and a link on two pins. Before solving, the six
rigid-body motions are projected through the supports, and a part they leave free is refused.

The viewer colours a `.gsim` by its result, using a 1D colour
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

`bearing housing:bore shaft`: the ties between the two parts are kept only within half the
coarser part's element size of the ball row and hold only the radial gap, each with an equal
share of the bearing's stiffness (twice the stiffness over the number of ties, since a ring of
radial springs is half as stiff in any one direction as their sum). With the bond penalty
instead, a band 4 mm long clamped the shaft like a pin and took 21 N·m of the moment on the
lever check below. With 1e5 N/mm the band's tilt stiffness is under 1% of a Ø20 shaft's over
160 mm. Thrust goes through one tie joining the mean axial movement of three shaft nodes at
120 degrees to three housing nodes at 120 degrees, so it acts on the axis and adds no tilt
stiffness; one node off the axis put 3 N·m of couple on a 300 N thrust at 10 mm. A Ø20 shaft
in two housings 160 mm apart with 1000 N on a 100 mm overhang reads 623 and 1623 N at mesh 4
(lever rule 625 and 1625) and 623.8 and 1623.8 N at mesh 2.5.

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
