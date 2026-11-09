# Standard parts and calculators

## Library parts

Fasteners come from the built-in library rather than a part file: `part s1 shcs:M3x10`
(also `fhcs:`, `hex:`, `nut:M3`, `washer:M3`, `insert:M3`, `standoff:M3x10`, `bearing:608`,
`dowel:4x16`, `oring:214`). Screw heads bear on z=0 with the shank down; mate `s1:shank.side`
and `s1:head.start`, nuts and washers by `bore.side`. Level a flat head with `distance
s:head.start lid:top.end 0`. The full table with each part's faces is in `docs/format.md`.

Size the holes they go in with the same name: `hole M3 x,y` (clearance), `cbore=M3`,
`csink=M3`, `fit=tap`, `fit=insert`. `oring_groove face 40 pressure=10` cuts the groove that
`gcad calc oring` works out.

## Calculators

`gcad calc` prints a result and a why line with its formulas and sources. Run it before drawing
the feature, then use the numbers.

```sh
gcad calc oring piston 50 --pressure 10 --formulas   # ring size, groove, squeeze, stretch
gcad calc thread M6 --material alu --depth 14        # drills, heads, torque, engagement
gcad calc fit H7/g6 25                               # ISO 286 limits for a hole and shaft
gcad calc material petg                              # properties and thread engagement
```

## Not calculators

Gears are drawn with `gear` (see [parts](parts.md)), not calculated. `gcad calc --help` lists every
calculator that exists.
