---
name: gcad
description: Create game assets with live preview and GLB or FBX/PNG export, and model mechanical parts and assemblies with gcad, a parametric CAD where every line of a `.gcad` part file or `.gasm` assembly file is one operation, like G-code, checked by the `gcad` CLI. Any design with more than one part is a `.gasm` assembly. Use when writing or editing `*.gcad`, `*.gasm`, `*.gsim` or `*.gdwg` files, sketching and extruding parts, adding holes, fillets, shells, gears or text, picking faces with selectors, placing parts with mates, joints, couples and patterns, checking interference, posing an assembly, bending sheet metal, framing weldments, picking standard parts and fasteners with `gcad calc`, sizing joint motors or running stress, heat, modal, flow, frame, conduction and acoustic simulations in a `.gsim`, making drawings and PDFs, exporting STEP, STL, 3MF, SVG, DXF, OBJ, GLB and a bill of materials, modelling organic mesh shapes such as characters, creatures and game assets from blended blobs and limbs sculpted with brushes and rigged with bones, and (as they land) building design with walls, slabs, roofs and stairs, and point clouds.
---

# gcad

A part is a text file of operations, one per line, run top to bottom against modal state the
way G-code runs against a machine: a workplane, a sketch, the solid, named faces. There are no
comments and no blocks. Every line is an operation, so you write the design by appending lines
and check it by running them.

```gcad
let w=40 h=30 t=3
rect w h r=4
base: extrude t
plane base.end
mounts: hole 3.2 15,10 -15,10 -15,-10 15,-10
fillet 0.8 base.end&base.side
```

**Read the reference file for your task before writing any line.** This file holds only what
every task shares.

| Task | Read |
|---|---|
| Sketches, extrudes, holes, fillets, workplanes, selectors, materials, enclosures, gears, text | [parts](references/parts.md) |
| More than one part: `.gasm`, mates, joints, couples, patterns, interference, posing, BOM | [assemblies](references/assemblies.md) |
| Laser-cut and bent sheet, welded tube and bar frames | [sheet-metal-weldments](references/sheet-metal-weldments.md) |
| Fasteners, o-rings, threads, fits, `gcad calc` | [standard-parts](references/standard-parts.md) |
| `.gsim`: torque, stress, heat, modal, flow, frame, conduction, acoustics, the sims server | [simulation](references/simulation.md) |
| `.gdwg` drawing sheets, views, sections, dimensions, labels | [drawings](references/drawings.md) |
| Characters, creatures, props, vehicles, buildings and other game assets, blobs, sculpting, rigging, clips | [meshes](references/meshes.md) |
| Game assets, `.gasset`, GLB and FBX for engines, runtime selection | [game-assets](references/game-assets.md) |
| Export and import: STEP, STL, 3MF, OBJ, GLB, SVG, DXF, PDF, BOM | [output](references/output.md) |
| Walls, openings, slabs, roofs, stairs, rooms | [architecture](references/architecture.md) |
| Scans, PLY, E57, LAS, registration, reconstruction | [point-clouds](references/point-clouds.md) |

Props, vehicles, weapons, furniture and buildings made for a game or a picture are not machined parts: read [meshes](references/meshes.md) and block them out with `blob` and `limb` lines that `blend`, not with `rect` and `extrude` alone. Use [parts](references/parts.md) only for pieces that will be cut, printed or fitted to other parts.

Anything made of more than one part is an assembly, even two parts: plan it as a `.gasm` from
the start. Sketch and solid operations are errors in a `.gasm`, joints are errors in a `.gcad`.

## Installing

Run `gcad --version` first. If it is missing:

```sh
curl -fsSL https://raw.githubusercontent.com/v0l/gcad/master/install.sh | sh   # Linux, macOS
```

On Windows take the zip from https://github.com/v0l/gcad/releases. Building from source is a
plain build: `Cargo.toml` pins a commit of the kernel fork's `cad-regressions` branch through
`[patch.crates-io]`, so cargo fetches it.

```sh
git clone https://github.com/v0l/gcad
cd gcad && cargo build --release
```

`gcad docs` prints the whole format: every operation with its parameters, the selectors, the
groups each operation records, and the assembly lines. Read the table for the operation you are
about to write before writing it. The references cover how to work, not every parameter.

## The loop

For an assembly, run the loop on each part file alone, then on the `.gasm`.

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

## Format rules

- `label: operation args name=value`. Label every line you will select from later. An unlabelled
  line is `L<number>`, which changes when lines move.
- Expressions have no spaces: `w/2-r`, `atan2(y,x)`, `sqrt(a*a+b*b)`. Angles are degrees.
  `let name=value` sets sizes; `--set name=value` overrides one.
- Selectors pick faces: `label`, `label.end`, `label.side`, `label.start`, `>Z`, `+Z`, `all`,
  and `a,b` for either. Edges are `a&b` and `x|y`. Details are in [parts](references/parts.md).
- Units are millimetres, newtons and megapascals unless a reference says otherwise.

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
- Fillets where three rounded edges meet need flat faces and straight edges. `shell` hollows the
  extrusion that owns the open face, so shell before adding other features.
- Boolean edges that are lines or circles (hole rims) are exact; other intersections, such as
  two crossing cylinders, are fine polylines.
- Assembly parts are built in parallel and cached per file and variables, so many copies of
  one part cost one build.
