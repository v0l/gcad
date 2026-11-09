# Parts

A `.gcad` file holds one part, or several parts as bodies when they share sizes, like a box and
the lid that fits it. Fenced `gcad` blocks in the references are real files that `gcad check`
runs in the test suite; `text` blocks are fragments that are not checked.

## Writing a part

- The sketch builds up from profile lines (`rect`, `circle`, `poly`, `ngon`, `slot`, `ellipse`,
  `spline`, `text`, `gear`, `pen` ... `close`) and is used up by the next `extrude`, `cut`,
  `revolve` or `sweep`. A profile inside another is a hole in it.
- `plane` sets where the next sketch goes. `plane base.end` puts it on a face; coordinates on a
  face stay world coordinates projected onto it.
- `extrude` adds, `cut` removes going into the solid, `hole` drills at a list of points.
  `mode=cut` and `mode=intersect` work on `revolve`, `sweep` and `loft`.
- `body name` starts another solid in the same file: a second part that shares this file's
  sizes, which an assembly brings in as `part.name`, or a piece you `combine` into this part.
  When a part file ends with several bodies, `check` reminds you to bring it into a `.gasm`.
- `color` and `material` on a body feed the STEP colours, the viewer and the mass in `bom`.

```gcad
let w=40 h=30 t=3 hole_d=3.2 pitch_x=30 pitch_y=20
rect w h r=4
base: extrude t
plane base.end
mounts: hole hole_d pitch_x/2,pitch_y/2 -pitch_x/2,pitch_y/2 -pitch_x/2,-pitch_y/2 pitch_x/2,-pitch_y/2
fillet 0.8 base.end&base.side
chamfer 0.3 mounts.side&base.end
```

## Selectors

Selectors pick faces: `label`, `label.end`, `label.side`, `label.start`, `>Z` (flat faces
furthest along +Z), `+Z` (every flat face facing +Z), `all`, and `a,b` for either. Edges are
`a&b` (where a face of `a` meets a face of `b`) and `x|y`. `base.end&base.side` is the top
rim of an extrusion and leaves hole rims alone. `gcad docs` lists which groups each operation
records.

## Materials

- Materials: the library has about 210 (steels, alloys, plastics, printing materials, timbers,
  concrete, brick, insulation, glass); `gcad materials` lists them, so run it before inventing
  one. Others go in a `materials.toml` beside the files or above them (`[petg-cf]`, `density =
  1.3`, `E = 3.8`, `nu = 0.36`, `yield = 52`; `E1 E2 E3 Xt Zt` for printed parts, axis 3 the
  print direction). `materials file.xml` or `file.FCMat` loads MatML or FreeCAD cards.
- The tables also set the look: `color`, `roughness`, `metallic` (1 only for bare metal; painted
  metal is 0), `clearcoat`, `transmission = 1` with `ior = 1.5` for glass, `texture = "wood"`
  with `texture_scale` in mm, or image maps `color_map`, `normal_map`, `roughness_map`,
  `metallic_map`. A `[steel]` table keeps every key of the standard steel it does not give.

## Gears

`gear teeth module` draws an involute spur gear with a tooth on `angle=` (default +x);
`gear teeth module internal` inside a larger `circle` makes a ring gear. Centres are
`module*(a+b)/2` apart outside, `module*(ring-gear)/2` inside. Two gears mesh when a tooth of
one points at a gap of the other: with a tooth on the line of centres on one, turn the other
by `180/teeth`. Use 17 or more teeth at 20 degrees; smaller pinions overlap their mate at the
root because undercut is not modelled. `interference none` catches a gear out of phase.
`gear ... helix=20` then `extrude` makes a helical gear (mate it with `helix=-20`), and
`gear ... cone=atan(a/b)` then `extrude` a bevel gear. `rack teeth module` draws a rack with its
pitch line on `at`; a gear meshes with its centre `pitch_diameter/2` above it.

```gcad
gear 20 2
g: extrude 5
plane g.end
hole 5 0,0
```

## Enclosures

`lip >Z 3` on the case rim and `groove <Z 3` on a lid shelled the same way and placed 3 lower;
`boss 7 20 x,y ... hole=M3 fit=insert ribs=4` for screw posts; `vent 20 3 x,y count=3
step=0,-5` cuts through the one wall under the workplane; `snap l t w x,y dir=` for clips. See
`examples/assemblies/case.gasm` and `examples/parts/enclosure.gcad`, whose case and lid are two
bodies sharing one set of sizes.

## Threads

`thread M6 on=rod.side` needs each end of the round face to be flat and square to it, or a
chamfer deeper than the thread (`chamfer 1 rod.end&rod.side` on an M6). Chamfer first and
thread last. Holes take `thread M6 on=h.side` after `h: hole 5 0,0 thread=M6`, with or
without `csink=`.

## Traps

- Inside and outside edges can meet at a fillet corner (pocket rims, boss feet, L shapes), but
  not at a corner whose third edge stays sharp. Fillets on a shelled part can come first, and
  the inside follows them at their radius less the wall.
- Fillet and chamfer selectors that match nothing fail the line; run `gcad query` first.

## Worked examples

- `examples/parts`: single parts with analytic-volume tests (`plate`, `bracket`, `pipe`,
  `ring`, `enclosure`).
