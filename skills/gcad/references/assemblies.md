# Assemblies

A `.gasm` file brings parts in from part files, places them with mates, joins them with joints
and checks that nothing overlaps. That holds for two parts as much as for thirty: a box and its
lid, a bracket and its bolts, a shaft and its gear. Check, render, export and `bom` the `.gasm`,
not a part file. A part file holds one part, or several as bodies; the `.gasm` brings each in as
`part.body`. Sketch and solid operations are errors in a `.gasm`, joints are errors in a `.gcad`.

Split a big design into sub-assemblies: `part az drive.gasm` brings one in with its parts,
joints and axes as `az.spindle`, `az.spin`, `az.wa`; `move az x,y,z` and `rotate az ...` place it
whole, and a `.gsim` can simulate the sub-assembly's file on its own.

Lay a design out as part files and one assembly:

```text
box/
  parts/enclosure.gcad   case and lid as two bodies, sized from one set of lets
  parts/screw.gcad
  box.gasm
```

Parts that share sizes can share a part file, one body each, so a change to `w` moves them
together. Parts that stand alone (fasteners, motors, a gear used in several places) get their
own file. The `.gasm` sets sizes on its `part` lines (`part case parts/enclosure.gcad w=80`).
A part can be drawn where it sits in the assembly, or at its own origin and placed with `move`,
`rotate` and mates. Reuse one part file for every copy, with different variables or `pattern`.

```text
part case ../parts/enclosure.gcad w=80
part s1 ../parts/screw.gcad
concentric s1:shank.side case.lid:screws.side near=32,17,30
flush s1:head.start case.lid:top.end
pattern s1 holes=case.lid:screws.side
axis hinge -40,25,30 40,25,30
joint lid case.lid case.main turn about=hinge min=-110 max=0
interference none
interference joint=lid steps=12
explode case.lid 0,0,30
```

## Placing and joining

- `part:faces` selects faces of a part with that part file's own labels.
- Place a part with `move` and `rotate` first, then mate it. The first mate of a part fixes it
  to the other part; later mates must already hold and are then kept. `flush` wants the faces
  to face each other.
- `joint NAME child parent turn about=AXIS` or `slide along=x,y,z`, with `min=` and `max=`.
  Define every joint with the assembly in its home position, then `pose` it at the end.
- `couple driven driver ratio=` makes one joint follow another: `-teeth_a/teeth_b` for a gear
  pair, `pi*d/360` mm per degree for a rack. A planetary sun turns `1+ring/sun` per turn of the
  carrier, a planet `-ring/planet` relative to it. Add losses with `efficiency=` (driven
  turning driver) and `back=` (the load turning it back), or for a worm `lead=` in degrees and
  `friction=` (0.05 to 0.15 greased), which works out both and whether it locks.
- `pattern part count=3 angle=360 axis=A` copies a part with its mates, joints and couples.
- A vendor STEP assembly comes in whole with `part box vendor.step`: each solid where the file
  puts it, named after its product.

## Interference

End the file with `interference none` and sweep each joint once with
`interference joint=NAME steps=12` to find the range where it clashes. Set `min`/`max` from
that, then keep the sweep as `interference none joint=NAME steps=N` so the check fails if a
later change clashes; without `none` a sweep only reports. Pick `N` so a step is not a whole
tooth pitch of any geared joint, or a gear pair out of phase never shows. A couple's gear and
pinion may share up to `module`³ where their teeth meet.

## Measuring

Size what goes inside before you draw it. `gcad measure case.gasm space at=x,y,z` grows the
largest empty box around a point and names what stops it on each side, then gives the free
distance from the point along each axis, which is the inside width. `gcad measure asm.gasm box`
lists every part's bounding box, and `clearance a b` the nearest gap between two parts and
where it is. None of these change the file.

## Bill of materials

`gcad bom asm.gasm` counts the parts (`--csv` for a spreadsheet); `gcad render asm.gasm x.png
--explode` draws it apart. Fasteners and other standard parts come from the built-in library,
see [standard-parts](standard-parts.md).

## Worked examples

- `examples/assemblies/enclosure.gasm`: the enclosure's case and lid hinged together, with screws
  mated into the lid and patterned into every hole; the screws stop the hinge.
- `examples/assemblies/linkage.gasm`: a four-bar linkage, a closed loop of joints.
- `examples/robot/arm.gasm`: a 34-part robot arm. Every joint is a motor on its axis driving a
  planetary reducer whose ring is cut into the housing; suns and planets are coupled at their
  ratios and the planets are patterned. The gripper is two racks on one pinion. Copy its
  patterns before inventing new ones. `examples/robot/arm.gsim` sizes its motors.
- `examples/truck/truck.gasm`: a 90-part 1:10 crawler. Four-link solid axles kept level by
  couples, coil-over shocks, telescoping driveshafts and the drag link and tie rod are closed
  loops that settle when `fsus`, `rsus` or `steer` is posed. Shared heights (`hi`, `rlo`) go
  from the `.gasm` into every part file. Fasteners are library screws and nuts placed with
  `concentric` and `distance` and patterned into hole sets.
