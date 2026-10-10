# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## Bonded assemblies do not converge

`cd ~/git/rotator/sims && gcad sim azframe.gsim` runs about 12 minutes and stops with
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
