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
gcad calc bearing 6205 --radial 2 --rpm 3000         # size, C, C0, seats, shoulders, L10 life
gcad parts list                                      # the standard parts catalogue
gcad parts show iso4762:M6x25                        # one part with its BOM line
```

Catalogue specs work as `part s iso4762:M6x25`; bought parts are `spec:name@WxDxH`.

## Not calculators

Gears are drawn with `gear` (see [parts](parts.md)), not calculated. `gcad calc --help` lists every
calculator that exists.

## Rules

1. Run `gcad calc` before drawing the feature. Never guess a groove, tap drill or fit.
2. Put every bought part in the model with `part name kind:size`, so `gcad bom` lists it.
3. Cite the standard in the answer (AS568 or ISO 3601, ISO 273, ISO 286, ISO 4762, ISO 15 for bearings).
4. `gcad check` prints `warning:` lines: a screw length that is not an ISO 888 length, or a tapped hole shorter than the minimum engagement. Fix the model, do not ignore them. See docs/standard-parts.md for the full list. Run `gcad check file --strict` to make warnings fail.
5. Size by hand only with a calculator number: `oring_groove face 40 depth=1.96 width=3.53`, `hole M6 x,y fit=tap depth=16`, a bearing shaft at the `gcad calc bearing` fit with a shoulder at its `shaft shoulder` size. Worked designs are in `examples/standard-parts/`.

## Worked example

```gcad
let d=40
rect 60 60
base: extrude 8
plane base.end
seal: oring_groove face d
```

`gcad check` reports `face groove 3.53 wide x 1.96 deep, ring ISO 3601 40x2.65`. In the assembly,
`part ring oring:40x2.65` puts the ring in the BOM.
