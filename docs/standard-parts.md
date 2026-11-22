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

`gcad check` prints `warning:` on the line that causes it. A warning never fails the build unless you run `gcad check file --strict`, which exits 1 when any warning printed.

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

A screw into a hole under its tap drill (run in `/tmp`, `part plate` has a 4 mm hole):

```
4 concentric s:shank.side plate:h.side  ok  moved s, now held by `plate`; radius 3.000 in 2.000; warning: the 4.00 mm hole is narrower than the 5.00 mm tap drill of M6, so the screw cannot enter it
```

## Worked designs

`examples/standard-parts/` holds three designs made only from `gcad calc`, `gcad parts` and the skill, each checking clean with `--strict`:

- `lid.gcad` and `lid.gasm`: an o-ring sealed lid. `gcad calc oring face 40 --pressure 5` gives ISO 3601 40x2.65, a 3.53 wide and 1.96 deep groove; `gcad calc thread M4` gives the 4.5 mm clearance holes (`hole M4`).
- `flange.gcad`: a flange with a tapped hole. `gcad calc thread M6 --material alu --depth 16` gives a 5 mm tap drill and 13 mm of thread against 12 mm needed in aluminium.
- `shaft.gcad` and `shaft.gasm`: a shaft with a collar, carrying a 6205. `gcad calc bearing 6205` gives a 25 mm bore with the shaft at m5 (25.008 to 25.017 mm) and a shoulder at least 30.6 mm across; the collar is 32 mm.

```
3 concentric b:bore.side shaft:shaft.side  ok  moved b, now held by `shaft`; radius 12.500 in 12.505
```
