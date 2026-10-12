# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## `elbox.gsim` meshes 30 times finer than it did

`~/git/rotator/sims/elbox.gsim` at `mesh 3` made 526 thousand nodes on gcad 0.2.0 and makes
15.7 million now (about 8 minutes and 50 GB). The box has a 0.05 mm deep `seat` cut, which the
mesh has to resolve; whether the growth comes from that is not known yet.

## Six board STEP solids still come in as boxes

`part board ctl.step` (from `agentee export ctl` in `~/git/rotator/board`) brings in 528 parts
with 6 boxes. Three fail the kernel's healing with "This wire is not simple": U1
(`Ag5400_simplified`), U2 (`SO-4_4.4x3.6mm_P2.54mm`) and part 2 of the RJ45
(`V8BR-1AX1-GH_PART2`). Three electrolytics (C2, C76, C83, models `CP_Elec_*`) use a
`SURFACE_OF_REVOLUTION` and `TOROIDAL_SURFACE` that `Surface::try_from` does not convert.
