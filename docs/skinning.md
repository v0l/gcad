# Skinning

How gcad binds a mesh to its bones, what makes a skin good, and the numbers that check it.

## Checking a skin

`gcad skincheck file.gcad` prints, for every skinned body:

| number | what it measures | good |
|---|---|---|
| weight jump | the largest change of any bone's weight across one mesh edge (max and 99th percentile), and how many edges change by more than 0.35 | no edge over 0.35 |
| contested | vertices whose two strongest bones are within 10% of each other | only in joint zones |
| influences | how many vertices have 0, 1, 2, 3 and 4 bones | at most 4, none with 0 |
| unrelated | vertices that take at least 5% from a bone that is not their strongest bone, its parent or its child | 0 |
| rigid core | for the middle 40% of each bone, the share of the vertices nearest that bone that are 99% or more on it | near 1 |
| posed quality | every clip pose at its keys and halfway between: the smallest and largest triangle area ratio against rest, the worst growth of the aspect ratio, and how many triangles flip, shrink below a quarter or grow past four times | no flips |

`tools/skin_shots.sh OUT` renders the fixtures at their worst keys with `gcad inspect` (set `GCAD` to the
binary to use).

## Fixtures

- `examples/mesh/rigged_creature.gcad`: the quadruped with a tail, `walk` and `idle` clips.
- `examples/mesh/skin_stress.gcad`: the same quadruped with the legs 0.4 closer, so the crotch is narrow,
  and a `rom` clip with the legs at 50 degrees, the knee at 70, the tail at 35 and the neck at 25.
- `examples/mesh/skin_arm.gcad`: a two-bone arm and a hand, bent 120 degrees at the elbow, swung 60 at
  the shoulder, and the hand twisted 90.

## Before (straight-line distance, `power=4`)

| fixture | max jump | p99 | edges over 0.35 | contested | unrelated | min core | area ratio | aspect growth | flipped | squashed | stretched |
|---|---|---|---|---|---|---|---|---|---|---|---|
| rigged_creature | 1.000 | 0.377 | 198 | 852 | 1084 | 0.00 | 0.056..90.8 | 28.3 | 182 | 14 | 141 |
| skin_stress | 0.813 | 0.271 | 107 | 794 | 1122 | 0.00 | 0.055..12.8 | 24.2 | 87 | 23 | 82 |
| skin_arm | 0.152 | 0.099 | 0 | 140 | 0 | 0.00 | 0.155..1.60 | 3.81 | 1469 | 166 | 0 |

The creature's hind legs and crotch fold into spikes in the walk because neighbouring vertices pick
different leading bones, and no bone has a rigid core: every vertex blends four bones by distance.
