# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## Flat caps on cylinder seams make sims mesh far too finely

`mesh_part` (src/sim/mesh.rs) retries at 0.6 of the size, up to five times, whenever the worst
dihedral is under 2 degrees. A shaft extruded as several `circle 20` segments end to end (like
`~/git/rotator/sims/elshaft_fea.gcad`) meshes at `mesh 4` in 6 s with 56 thousand tetrahedra,
but keeps 0.0 degree slivers at every size from 3.6 to 6, so the retries go down to about 0.5 mm
and the sim gets 1.2 million tetrahedra after two minutes. The slivers have all four corners on
the surface, at the 45 degree seam lines of the cylinder faces. `shed_flat_caps`
(src/sim/tet/mod.rs) refuses 132 of them because shedding would push the cap's opposite edge
past two surface triangles (the rim check). The same shaft with its bands at `circle 24` meshes
cleanly. Repro: five extrudes of `circle 20` 10, 4, 86, 4, 86 mm long, `fix a.start`, `mesh 4`,
`gcad sim x.gsim --dry-run`.

## `elbox.gsim` meshes 30 times finer than it did

`~/git/rotator/sims/elbox.gsim` at `mesh 3` made 526 thousand nodes on gcad 0.2.0 and makes
15.7 million now (about 8 minutes and 50 GB). The box has a 0.05 mm deep `seat` cut, which the
mesh has to resolve; whether the growth comes from that or from the stacked-cylinder fault above
is not known yet.

## Six board STEP solids still come in as boxes

`part board ctl.step` (from `agentee export ctl` in `~/git/rotator/board`) brings in 528 parts
with 6 boxes. Three fail the kernel's healing with "This wire is not simple": U1
(`Ag5400_simplified`), U2 (`SO-4_4.4x3.6mm_P2.54mm`) and part 2 of the RJ45
(`V8BR-1AX1-GH_PART2`). Three electrolytics (C2, C76, C83, models `CP_Elec_*`) use a
`SURFACE_OF_REVOLUTION` and `TOROIDAL_SURFACE` that `Surface::try_from` does not convert.
