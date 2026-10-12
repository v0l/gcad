# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## `pin` on a short cylinder band acts as a clamp

In `sims/elshaft.gsim` the 2 mm bearing bands are `pin`ned, yet the reaction moments are
14 and 29 N·m. A bearing seat should carry force and almost no moment.

## Importing a KiCad/agentee board STEP

Importing the STEP from `agentee export ctl -o ctl.step` (run in `~/git/rotator/board`)
fails with "This wire is not simple" and boolean errors. As a workaround,
`~/git/rotator/board/tools/board_gcad.py` turns each part into its bounding box.

## Revolved screws do not mesh

`parts/std/screwcsk.gcad d=4 L=16` in the rotator (a profile revolved about an axis it
touches) fails at every `mesh` size with "could not recover 4 surface triangle(s)". The
head and tip discs are parameterised with a pole on the axis: the face CDT puts several
points within 1e-6 of it, and the recovery loop keeps splitting the slivers there.
Welding interior CDT points and collapsing points within 1e-6 of each other did not help,
because the splits add new ones. Mesh planar faces in their own plane instead of the
surface's uv. Until then, `leave *screw*` keeps such parts out of a sim.
