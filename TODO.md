# TODO

Problems found while sizing the rotator (`~/git/rotator`, `sims/`) with `gcad sim`. Each
item has a way to reproduce it. Take one, fix it, delete it from this file.

## Bonds reach across clearances

Parts bond wherever they come within a tenth of their element size (`src/sim/statics.rs:256`),
so a cover with a small clearance gets glued to the part it covers. In
`~/git/rotator/sims/rotator.gsim`, `cover_el` bonds to `wheel_el` at 29,830 nodes and `cover_az` to
`wheel_az` at 5,049, giving the gear covers a load path through the worm wheels (unverified
that the clearance is what is bridged; measure the gap in `parts/gearcover.gcad`). The covers
and the yagi parts together take the solve from 592 to 1,372 iterations; the 2026-10-11
rotator (`030f6dd`) still solves in 550 with today's gcad, so the solver itself is fine. Bond only
faces that touch, or list bonded pairs whose faces sit apart.

## Five board STEP bodies still come in as boxes

All fail the kernel's healing (`monstertruck-healing`), so they need fork work. In
`~/git/rotator/board/models/Ag5400_simplified.step` (123 bodies written as voids of one pin),
three bodies are "not oriented and closed" and one has a wire that "is not simple". Part 2 of
`~/git/rotator/board/models/V8BR-1AX1-GH.stp` (the RJ45) has a wire that "is not simple". Check
with `part m <file>` in a `.gasm`; OpenCascade reads all of them.
