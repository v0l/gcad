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
| engagement in this material | an internal `thread` shorter than the `material` needs (steel 1.25, aluminium 2, plastic 2.5 diameters, class 8.8) |
| o-ring squeeze or fill | `oring_groove face d depth= width=` cuts a hand-sized face groove whose squeeze or fill is outside the calculator's range |
| narrower than the tap drill | `concentric` puts an ISO screw in a hole narrower than its tap drill |
| unknown standard, no such nut or washer size | `part` line fails (an error, not a warning) and lists the valid kinds or sizes |
| bearing seat | `concentric` puts a catalogue bearing on a shaft or in a bore outside the fit classes of `gcad calc bearing` |
| bearing without a shoulder | a catalogue bearing sits on a shaft or in a bore with no shoulder within 0.5 mm of either face, or a shoulder lower than the catalogue da min (shaft) or higher than Da max (housing) |

```
3 part s iso4762:M6x27  ok  s from iso4762:M6x27; warning: 27 mm is not a standard length for M6 (iso4762); the nearest ISO 888 length is 25 mm
```
