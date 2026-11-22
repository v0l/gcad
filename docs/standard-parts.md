# Standard parts and sizing

gcad sizes seals, threads, fits and bearings with `gcad calc`, holds bought parts in a catalogue
(`gcad parts list`), and `gcad check` warns when a model looks wrong for a declared standard part.

## Calculators

| command | gives | source |
|---|---|---|
| `gcad calc oring face 40 --pressure 5` | ring, groove width and depth, squeeze, fill | Parker ORD 5700 charts 4-2, 4-3, 5-2-a; AS568, ISO 3601-1 |
| `gcad calc thread M6 --material alu --depth 14` | tap drill, clearance, heads, torque, engagement | ISO 273, ISO 262, ISO 898-1 |
| `gcad calc fit H7/g6 25` | hole and shaft limits, clearance | ISO 286-1 |
| `gcad calc bearing 6205 --radial 2 --rpm 3000` | seats, shoulders, L10 life | ISO 15, SKF catalogue, ISO 281 |

## Standard parts in a model

`part s iso4762:M6x25`, `part r oring:40x2.65`, `part b bearing:6205` bring catalogue parts into
a `.gasm`, so `gcad bom` lists them with a material and mass. See `gcad parts list`.

## Check warnings

`gcad check` prints `warning:` on the line that causes it. A warning never fails the build.

| warning | when |
|---|---|
| not a standard length | a screw spec such as `iso4762:M6x27` whose length is not an ISO 888 length |
| minimum engagement | a `thread` on a hole shorter than one diameter |

```
3 part s iso4762:M6x27  ok  s from iso4762:M6x27; warning: 27 mm is not a standard length for M6 (iso4762); the nearest ISO 888 length is 25 mm
```
