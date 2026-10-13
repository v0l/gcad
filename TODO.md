# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## `elbox.gsim` meshes 30 times finer than it did

`~/git/rotator/sims/elbox.gsim` at `mesh 3` made 526 thousand nodes on gcad 0.2.0 and makes
15.7 million now (about 8 minutes and 50 GB). The box has a 0.05 mm deep `seat` cut, which the
mesh has to resolve; whether the growth comes from that is not known yet.

## Five board STEP bodies still come in as boxes

All fail the kernel's healing (`monstertruck-healing`), so they need fork work. In
`~/git/rotator/board/models/Ag5400_simplified.step` (123 bodies written as voids of one pin),
three bodies are "not oriented and closed" and one has a wire that "is not simple". Part 2 of
`~/git/rotator/board/models/V8BR-1AX1-GH.stp` (the RJ45) has a wire that "is not simple". Check
with `part m <file>` in a `.gasm`; OpenCascade reads all of them.
