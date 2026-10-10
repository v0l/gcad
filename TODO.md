# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## Bonded joints leak moment

A bonded assembly is stiffer than the same shape as one solid, and its reactions miss part
of the applied moment, so the solution is out of equilibrium. Both files below are the
same shape: a 13 x 13 square rod 107 long, with a Ø44 flange and a Ø24 post to z 222 that
the rod's top 20 mm sits in.

```
sq.gcad:   rect 13 13 / r: extrude 107 / material steel
hq.gcad:   plane XY offset=87 / circle 44 / f: extrude 8 / plane f.end / circle 24
           post: extrude 127 / plane XY offset=107 / rect 13 13 / bore: cut 20 / material steel
sq.gasm:   part s sq.gcad / part h hq.gcad
sq.gsim:   sim static / part sq.gasm / mesh 4 / root: fix s:r.start
           load h:post.end force=0,100,0
```

| model | root moment | tip deflection |
|---|---|---|
| one solid | 22.200 N·m (exact) | 0.62 mm |
| bonded, `mesh 4` | 11.9 N·m | 0.28 mm |
| bonded, `mesh 3` | 17.5 N·m | |
| bonded, `mesh 4`, head fixed, rod loaded | 18.5 N·m | |

A round Ø15 rod in a bore does the same: 9.1 N·m at `mesh 4`, 16.6 at 2, 19.1 at 1.5.
The force still balances (100.0 N), and two rods bonded end to end are right (9.997 N·m),
so the leak is in the ties along the side faces. Something in them resists rigid rotation.
Unverified guess: `Tie` constrains u_s - Σ w u_m with an isotropic penalty, and somewhere
x_s ≠ Σ w x_m. The docs' checks (bonded stack in tension, two-material wall) only load
in translation and would not catch it.

Every bonded static result is suspect until this is fixed. The rotator's az frame and
column sims were dropped for this reason.

## Bonded assemblies do not converge

`gcad sim azframe.gsim` (rotator commit `cc02346`, `sims/`) runs about 12 minutes and stops with
"the solver did not converge; check the supports hold the part"
(`src/sim/fem/elastic.rs:621`). It is a mast clamp, a 3 mm slab, four steel standoffs and
a plate, all bonded, with the clamp bore fixed.

- With three posts (rotator commit `c3f3f6b`) it converged at `mesh 7`, after 2300 to 2500
  iterations, but not at `mesh 3` or `mesh 4`. With four posts it fails at 6, 7 and 8.
- The Hammond box body and its lid bonded together, with the collar face fixed, also
  failed after 12 minutes. The body on its own (`sims/elbox.gsim`) solves in 8 s and 36
  iterations.

The parts are held, so the message points the wrong way. Unverified guess: the bonded ties
spoil the conditioning or the coarse space.

## The headline safety comes from singular peaks

`sims/elbox.gsim` reports a peak of 214325 MPa at the edge of a fixed face, and
`azframe.gsim` reports 140615 MPa where a post is bonded to the plate. Probes away from
those spots read 3 to 30 MPa. `safety` is therefore useless and `expect safety>2` can't
be used on assemblies. Leave out a band at fixed-face edges and bond interfaces, or report
that peak separately.

## `pin` on a short cylinder band acts as a clamp

In `sims/elshaft.gsim` the 2 mm bearing bands are `pin`ned, yet the reaction moments are
14 and 29 N·m. A bearing seat should carry force and almost no moment.

## `include` does not bring in `let`

```
inc.gsim:  let F=10
t.gsim:    sim static / include inc.gsim / ... / load b.end force=F,0,0
```

This fails with "`F` is not defined". Every rotator gsim repeats the same load `let`
block for this reason.

## `torque ... axis=z`

"`z` is not a 3D point, write it as x,y,z". Accept `x`, `y` and `z` as axes, the same way
`rotate ... axis=` does.

## Probes on a surface

A probe has to lie strictly inside the solid, so a point on a face is rejected as "not
inside the part". Snap it to the nearest element within a small tolerance and say so.

## Importing a KiCad/agentee board STEP

Importing the STEP from `agentee export ctl -o ctl.step` (run in `~/git/rotator/board`)
fails with "This wire is not simple" and boolean errors. As a workaround,
`~/git/rotator/board/tools/board_gcad.py` turns each part into its bounding box.
