# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## The whole rotator takes three times the iterations it did

`~/git/rotator/sims/rotator.gsim` solved in 480 conjugate gradient iterations on 2026-10-11 (2.8
million nodes) and takes 1,372 now (4.5 million nodes, 22 minutes). The residual first climbs
to about 470 times its start before it falls. Whether the model grew (joint drag, bearings, more
parts) or the preconditioner got worse is not known; solving the 2026-10-11 rotator at its own
commit with today's gcad would tell.

## Five board STEP bodies still come in as boxes

All fail the kernel's healing (`monstertruck-healing`), so they need fork work. In
`~/git/rotator/board/models/Ag5400_simplified.step` (123 bodies written as voids of one pin),
three bodies are "not oriented and closed" and one has a wire that "is not simple". Part 2 of
`~/git/rotator/board/models/V8BR-1AX1-GH.stp` (the RJ45) has a wire that "is not simple". Check
with `part m <file>` in a `.gasm`; OpenCascade reads all of them.
