# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

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
