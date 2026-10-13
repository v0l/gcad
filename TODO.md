# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## `elbox.gsim` meshes 30 times finer than it did

`~/git/rotator/sims/elbox.gsim` at `mesh 3` made 526 thousand nodes on gcad 0.2.0 and makes
15.7 million now (about 8 minutes and 50 GB). The box has a 0.05 mm deep `seat` cut, which the
mesh has to resolve; whether the growth comes from that is not known yet.

## Three board STEP solids still come in as boxes

`part board ctl.step` (from `agentee export ctl` in `~/git/rotator/board`) brings in 528 parts
with 3 boxes. All three fail the kernel's healing with "This wire is not simple": U1
(`~/git/rotator/board/models/Ag5400_simplified.step`), U2 (`SO-4_4.4x3.6mm_P2.54mm`) and
part 2 of the RJ45 (`~/git/rotator/board/models/V8BR-1AX1-GH.stp`).
