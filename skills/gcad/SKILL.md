---
name: gcad
description: Model mechanical parts and assemblies with gcad, a parametric CAD where every line of a `.gcad` part file or `.gasm` assembly file is one operation, like G-code, checked by the `gcad` CLI. Use when writing or editing `*.gcad` or `*.gasm` files, sketching and extruding parts, adding holes, fillets, shells, gears or text, picking faces with selectors, placing parts with mates, joints, couples and patterns, checking interference, posing an assembly, or exporting STEP, STL, 3MF, SVG drawings and a bill of materials.
---

# gcad

A part is a text file of operations, one per line, run top to bottom against modal state the
way G-code runs against a machine: a workplane, a sketch, the solid, named faces. There are no
comments and no blocks. Every line is an operation, so you write the design by appending lines
and check it by running them.

```
let w=40 h=30 t=3
rect w h r=4
base: extrude t
plane base.end
mounts: hole 3.2 15,10 -15,10 -15,-10 15,-10
fillet 0.8 base.end&base.side
```

Part files (`.gcad`) build one or more solid bodies. Assembly files (`.gasm`) bring parts in,
place them with mates, join them with joints and check that nothing overlaps. Neither has any
geometry the other can make: sketch and solid operations are errors in a `.gasm`, joints are
errors in a `.gcad`.

## Installing

Run `gcad --version` first. If it is missing:

```sh
curl -fsSL https://raw.githubusercontent.com/v0l/gcad/master/install.sh | sh   # Linux, macOS
```

On Windows take the zip from https://github.com/v0l/gcad/releases. Building from source needs
the kernel fork checked out beside it, because `Cargo.toml` patches it in by path:

```sh
git clone https://github.com/v0l/gcad
git clone -b cad-regressions https://github.com/v0l/monstertruck
cd gcad && cargo build --release
```

## Reference

`gcad docs` prints the whole format: every operation with its parameters, the selectors, the
groups each operation records, and the assembly lines. Read the table for the operation you are
about to write before writing it. This skill covers how to work, not every parameter.

## The loop

1. Add or change a few lines.
2. `gcad check part.gcad`. It runs every line and prints what each did: the profile count after
   a sketch line, the face count, volume and bounding box after a solid line, the plane origin
   and axes after a `plane`. It stops at the first failing line, prints why, and exits 1.
   `--set w=50` overrides a `let`, `--time` adds how long each line took.
3. `gcad render part.gcad /tmp/p.png` and look at the PNG (iso, top, front from -Y, right from
   +X). A clean check does not mean the part is right: a hole on the wrong face or a fillet on
   the wrong edges only shows in the picture.
4. `gcad query part.gcad 'base.end&base.side' --line 5` before any `fillet`, `chamfer` or
   `plane` whose selector you are not sure of. It lists the faces or edges it matches.

Read the volume and bounding box after every solid line and compare them with what you
expected. That catches most mistakes one line after you make them: a cut that removed nothing,
an extrude that went the wrong way, a hole that missed.

`gcad part.gcad` opens the viewer, which rebuilds on every save. Start it for a human who is
watching; you work from `check` and `render`.

## Writing a part

- `label: operation args name=value`. Label every line you will select from later. An unlabelled
  line is `L<number>`, which changes when lines move.
- Expressions have no spaces: `w/2-r`, `atan2(y,x)`, `sqrt(a*a+b*b)`. Angles are degrees.
- The sketch builds up from profile lines (`rect`, `circle`, `poly`, `ngon`, `slot`, `ellipse`,
  `spline`, `text`, `gear`, `pen` ... `close`) and is used up by the next `extrude`, `cut`,
  `revolve` or `sweep`. A profile inside another is a hole in it.
- `plane` sets where the next sketch goes. `plane base.end` puts it on a face; coordinates on a
  face stay world coordinates projected onto it.
- `extrude` adds, `cut` removes going into the solid, `hole` drills at a list of points.
  `mode=cut` and `mode=intersect` work on `revolve`, `sweep` and `loft`.
- `body name` starts a second solid in the same file; an assembly brings it in as
  `part.name`.
- `color` and `material` on a body feed the STEP colours, the viewer and the mass in `bom`.

Selectors pick faces: `label`, `label.end`, `label.side`, `label.start`, `>Z` (flat faces
furthest along +Z), `+Z` (every flat face facing +Z), `all`, and `a,b` for either. Edges are
`a&b` (where a face of `a` meets a face of `b`) and `x|y`. `base.end&base.side` is the top
rim of an extrusion and leaves hole rims alone. `gcad docs` lists which groups each operation
records.

## Assemblies

```
part case ../parts/enclosure.gcad w=80
part s1 ../parts/screw.gcad
concentric s1:shank.side case.lid:screws.side near=32,17,30
flush s1:head.start case.lid:top.end
pattern s1 holes=case.lid:screws.side
axis hinge -40,25,30 40,25,30
joint lid case.lid case.main turn about=hinge min=-110 max=0
pose lid -30
interference none
explode case.lid 0,0,30
```

- `part:faces` selects faces of a part with that part file's own labels.
- Place a part with `move` and `rotate` first, then mate it. The first mate of a part fixes it
  to the other part; later mates must already hold and are then kept. `flush` wants the faces
  to face each other.
- `joint NAME child parent turn about=AXIS` or `slide along=x,y,z`, with `min=` and `max=`.
  Define every joint with the assembly in its home position, then `pose` it at the end.
- `couple driven driver ratio=` makes one joint follow another: `-teeth_a/teeth_b` for a gear
  pair, `pi*d/360` mm per degree for a rack. A planetary sun turns `1+ring/sun` per turn of the
  carrier, a planet `-ring/planet` relative to it.
- `pattern part count=3 angle=360 axis=A` copies a part with its mates, joints and couples.
- End the file with `interference none` and sweep each joint once with
  `interference joint=NAME steps=12` to find the range where it clashes. Set `min`/`max` from
  that.
- `gcad bom asm.gasm` counts the parts; `gcad render asm.gasm x.png --explode` draws it apart.

## Gears

`gear teeth module` draws an involute spur gear with a tooth on `angle=` (default +x);
`gear teeth module internal` inside a larger `circle` makes a ring gear. Centres are
`module*(a+b)/2` apart outside, `module*(ring-gear)/2` inside. Two gears mesh when a tooth of
one points at a gap of the other: with a tooth on the line of centres on one, turn the other
by `180/teeth`. Use 17 or more teeth at 20 degrees; smaller pinions overlap their mate at the
root because undercut is not modelled. `interference none` catches a gear out of phase.

## Output

```sh
gcad export part.gcad part.step          # also .stl .obj .3mf
gcad export part.gcad part.svg --section y=0   # four-view drawing with a hatched section
gcad export asm.gasm asm.step            # one product per part file, instances placed
gcad bom asm.gasm --csv
```

## Traps

- `XZ` has normal -Y, so `plane XZ offset=12` sits at y=-12, and `extrude -36` from it goes
  toward +Y. Read the plane line `check` prints before extruding from it.
- On a face whose outward normal is +Y the plane's x runs along -X (it is "looking at the face
  from outside, Z up"). Coordinates on it are mirrored; prefer the opposite face or a world
  plane for holes on such faces.
- `hole` and `cut` go into the solid against the plane normal. A hole whose plane faces away
  from the solid removes nothing and the line fails or the volume does not change.
- Booleans are the slow and fragile part of the kernel. Unions of faces lying in one plane work,
  but when one fails ("not oriented and closed"), draw the shape as one profile instead of
  adding overlapping pieces one at a time.
- Fillets where three rounded edges meet need flat faces and straight edges. Inside and
  outside edges can meet there (pocket rims, boss feet, L shapes), but not at a corner
  whose third edge stays sharp. `shell` hollows the extrusion that owns the open face,
  so shell before adding other features.
- Hole edges made by booleans are fine polylines, not exact circles.
- Assembly parts are built in parallel and cached per file and variables, so many copies of
  one part cost one build.

## Worked examples

- `examples/parts`: single parts with analytic-volume tests (`plate`, `bracket`, `pipe`,
  `ring`, `enclosure`).
- `examples/assemblies/enclosure.gasm`: a hinged box with screws mated into the lid and
  patterned into every hole; the screws stop the hinge.
- `examples/assemblies/linkage.gasm`: a four-bar linkage, a closed loop of joints.
- `examples/robot/arm.gasm`: a 34-part robot arm. Every joint is a motor on its axis driving a
  planetary reducer whose ring is cut into the housing; suns and planets are coupled at their
  ratios and the planets are patterned. The gripper is two racks on one pinion. Copy its
  patterns before inventing new ones.
