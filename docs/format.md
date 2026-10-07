# The linecad format

A `.lcad` file is a list of operations, one per line, run from top to bottom. Every
line is an operation; there are no comments and no blocks. Blank lines are skipped.

```
let w=40 h=30 t=3
rect w h r=4
base: extrude t
plane base.end
mounts: hole 3.2 15,10 -15,10 -15,-10 15,-10
fillet 0.8 base.end&base.side
chamfer 0.3 mounts.side&base.end
```

## Line shape

```
[label:] operation [argument ...] [name=value ...]
```

- `label:` names the faces the line creates, so later lines can select them. A line
  without a label is named `L<line number>`, which changes when lines move, so label
  anything you will refer to.
- Plain arguments fill the operation's parameters in order. Any parameter can also be
  given as `name=value`.
- Numbers are expressions: `+ - * /`, parentheses, `pi`, and names set by `let`.
  Expressions cannot contain spaces.
- A 2D point is `x,y`, a 3D point is `x,y,z`, each part an expression.
- Lengths are millimetres, angles are degrees.

## State

The file runs against these pieces of state, the way G-code runs against a machine:

| state | set by | used by |
|---|---|---|
| variables | `let` | every expression |
| workplane | `plane` (XY until set) | sketch operations, `extrude`, `cut`, `revolve`, `hole`, `helix`, `repeat` |
| sketch | `rect`, `circle`, `poly`, `ngon`, `slot`, `ellipse`, `spline`, `text`, `pen` ... `close` | `extrude`, `cut`, `revolve`, `sweep`, `section`, which empty it |
| sections | `section` | `loft` |
| path | `path`, `helix` | `sweep`, `plane path`, `repeat along=path` |
| points | `point`, `dist`, `horizontal`, `vertical`, `angle` | any 2D point |
| axes | `axis` | `revolve`, `rotate` |
| solid | every operation that makes or removes material | `fillet`, `chamfer`, `shell`, `draft`, `push`, selectors |
| bodies | `body` | export and render, which take every body |

## Operations

Variables, conditions and other files:

| operation | parameters | does |
|---|---|---|
| `let` | `name=expr ...` | sets variables, in order, so later pairs can use earlier ones; a variable given with `--set` keeps its outside value |
| `if` | `condition operation ...` | runs the rest of the line only when the condition is not zero; `< > <= >= == !=` give 1 or 0 |
| `include` | `file.lcad` `name=value ...` | runs another file's lines here, with those variables fixed; the file is relative to this one |

Sketch:

| operation | parameters | does |
|---|---|---|
| `plane` | `on` `offset=` `rx=` `ry=` `rz=` | sets the workplane to `XY`, `XZ`, `YZ` or a flat face selector, moved `offset` along its normal, then turned about its own x, y and normal axes by degrees |
| `plane` | `x,y,z x,y,z x,y,z` | the plane through three points, x along the first two |
| `plane` | `edge=a&b` `angle=` | the plane through a straight edge, lying on face `a` at 0 and turning toward face `b` |
| `plane` | `path` `at=` | the plane square to the path, `at` a fraction of its length |
| `axis` | `name x,y,z x,y,z` | names the line through two points, for `revolve axis=` and `rotate axis=` |
| `rect` | `w h` `at=x,y` `r=` | adds a rectangle centred on `at`, corners rounded by `r` |
| `circle` | `d` `at=x,y` | adds a circle |
| `ellipse` | `dx dy` `at=x,y` | adds an ellipse of those diameters |
| `poly` | `x,y x,y x,y ...` `r=` or `c=` | adds a closed polygon, corners rounded by `r` or cut by `c` |
| `ngon` | `d n` `at=x,y` `angle=` | adds a regular polygon of `n` sides with corners on a circle of diameter `d` |
| `slot` | `l w` `at=x,y` `angle=` | adds a slot `l` long overall and `w` wide, turned by `angle` degrees |
| `spline` | `x,y x,y ... closed` | adds a smooth closed curve through the points |
| `text` | `"words"` `size=` `at=x,y` | adds the outlines of the words, `size` tall, starting at `at` |
| `offset` | `d` | adds a copy of the last profile grown by `d` (shrunk when negative) |
| `reflect` | `x`, `y` or `x,y x,y` | adds the last profile mirrored across the sketch's x or y axis, or the line through two points |
| `array` | `count=` and `angle=` `at=x,y`, or `step=x,y` | adds copies of the last profile around `at` or in a row |
| `pen` | `x,y` | starts a path of lines and arcs at a point |
| `line` | `x,y` | draws a straight segment to the point |
| `arc` | `x,y` `via=x,y`, or `x,y center=x,y turn=cw\|ccw` | draws an arc to the point through `via`, or about a centre |
| `dxf` | `file.dxf` `at=x,y` `scale=` | adds the closed outlines in a DXF (lines, arcs, circles, polylines) |
| `svg` | `file.svg` `at=x,y` `scale=` | adds the closed shapes in an SVG, one user unit to a millimetre, with y turned up |
| `close` | `trim` | closes the path back to its start and adds it to the sketch; `trim` instead cuts it where its last line crosses an earlier one |

Any profile line can end with `construct`: the profile is kept for reference and
left out of the sketch.

Sketch points and constraints:

| operation | parameters | does |
|---|---|---|
| `point` | `name x,y`, or `name` `near=x,y` | adds a fixed point, or a free one the constraints place |
| `dist` | `a b d` | the points are `d` apart |
| `horizontal`, `vertical` | `a b` | the points are level, or one above the other |
| `angle` | `a b degrees` | the line from `a` to `b` points that way |
| `coincident` | `a b` | the points are the same point |
| `parallel`, `perpendicular` | `a b c d` | line `a b` is parallel or square to line `c d` |
| `equal` | `a b c d` | line `a b` is as long as line `c d` |
| `midpoint` | `m a b` | `m` is halfway between `a` and `b` |
| `online` | `p a b` | `p` is on the line through `a` and `b` |

After every point and constraint line the points are solved and `check` prints them
and the degrees of freedom left. A point name works anywhere a 2D point does, and
`a.x`, `a.y` work in expressions.

Features:

| operation | parameters | does |
|---|---|---|
| `extrude` | `d`, `next` or `upto=faces` `offset=`, `both`, `draft=`, `thin=`, `mode=add\|cut\|intersect` | adds the sketch along the plane normal (negative `d` goes the other way); `both` centres it on the plane; `upto` stops at a flat face, `offset` past it; `next` stops at the first face the sketch reaches; `draft` tapers it inward by degrees; `thin` makes walls that thick inside the profile |
| `cut` | `d` or `thru`, `draft=` | removes the sketch from the solid, going into it against the plane normal |
| `revolve` | `angle` `axis=x\|y\|name` `mode=` | spins the sketch about the workplane's x or y axis through its origin, or a named `axis` |
| `path` | `x,y,z x,y,z ...` `r=` or `smooth` | sets the sweep path in world coordinates; corners are bent with radius `r`, or `smooth` makes one curve through the points |
| `helix` | `r= pitch= turns=` `at=x,y` | sets a helical sweep path about the workplane normal |
| `sweep` | `mode=` `twist=` `scale=` | carries the sketch from the path start along the path, turning it `twist` degrees and sizing it to `scale` by the end |
| `section` | | stores the sketch as one cross-section for `loft` |
| `loft` | `smooth` `mode=` | joins the stored sections with ruled faces, or one smooth surface through them all with `smooth`; sections with different edge counts are matched up by splitting edges |
| `hole` | `d x,y ...` `depth=` `cbore=d,depth` `csink=d,angle` `thread=M4` `on=label.side` | drills holes at each point, through unless `depth` is given, with an optional counterbore or countersink; `thread` checks the drill suits the tap and records it; with `on=` the points are angle,height on the side of an extruded circle and the holes go in square to it |
| `shell` | `t` `open=faces,...` | hollows an extrusion (open `label.end`, `label.start` or both) or a full revolve (open its flat `label.caps`) to walls `t` thick |
| `draft` | `angle faces` `neutral=face` | tilts flat side faces inward by `angle` degrees, hinged where they meet the neutral face; `label.side` of an extrusion that is the whole solid tapers curved sides too |
| `push` | `faces d` | moves flat faces `d` along their normal, out (positive) or in (negative); an extrusion's `label.end` or `label.start` moves in even with curved sides |
| `rib` | `t` | after an open `pen` path of one `line`, fills between that line and the solid with a web `t` thick, centred on the workplane |
| `thicken` | `faces t` | grows every side of an extrusion (`label.side`) outward by `t`, or pushes flat faces out by `t` |
| `fillet` | `size edges` `to=` | rounds the edges with radius `size`, or from `size` to `to` along them (on a closed loop, up to `to` halfway round and back) |
| `fillet` | `full label.end` | rounds the end of an extruded rect into a half cylinder across its short side, while the extrusion is the whole solid |
| `chamfer` | `size edges` `d2=` | bevels the edges by `size`; with `d2`, the edges must be written `a&b` and `size` is cut along `a`, `d2` along `b` |

`mode=cut` and `mode=intersect` on `revolve`, `sweep` and `loft` remove the shape or
keep only what it shares with the solid.

Bodies:

| operation | parameters | does |
|---|---|---|
| `mirror` | `on` `offset=` `of=label` | adds the mirror image of the solid, or of one labelled feature, across a named plane or flat face |
| `repeat` | `label` `count=` and `step=x,y`, `angle=` or `along=path` | repeats what a labelled line added or removed, in a row along the workplane, around its normal, or spaced along the path |
| `move` | `x,y,z` `copy` | moves the solid, or adds a moved copy |
| `rotate` | `angle` `axis=x\|y\|z\|name` `about=x,y,z` `copy` | turns the solid, or adds a turned copy |
| `scale` | `factor` or `x,y,z` `about=x,y,z` | scales the solid, evenly or per axis |
| `split` | `on` `offset=` `keep=below\|above` | cuts the solid with a plane and keeps one side |
| `body` | `name` | sets the current solid aside and starts a new one; the first body is `main` |
| `combine` | `into from` `mode=add\|cut\|intersect` | joins body `from` into body `into`, or cuts it out, and drops `from` |
| `place` | `body` `on=faces` | moves the body so its lowest point sits on the faces |
| `import` | `file.step` or `file.stl` | adds the solids in a STEP file, or a watertight STL mesh with its flat regions merged into faces, relative to the `.lcad` file |
| `color` | name, `#rrggbb` or `r,g,b` | colours the current body in STEP and 3MF files |

Assemblies. Each `body` is a part. Joints hang one body off another, so moving a
joint moves that body and everything hanging off it:

| operation | parameters | does |
|---|---|---|
| `joint` | `name child parent turn about=axis` or `slide along=x,y,z`, `min=` `max=` `at=` | joins two bodies; a `turn` joint spins the child about a datum `axis` by degrees, a `slide` joint moves it along a direction by millimetres; `at` poses it now |
| `pose` | `joint value` | moves a joint to a new value |
| `interference` | `none`, `joint=` `steps=` | reports the volume each pair of bodies shares; `none` fails the line if any do; `joint=` checks across the joint's range in `steps` |

Build each part first and add joints at the end: posing moves the solids but not the
face groups their selectors use.

Measuring, which changes nothing and prints the answer:

| operation | parameters | prints |
|---|---|---|
| `measure` | `faces faces` | the distance between two sets of faces |
| `measure` | `mass` | volume, surface area and centroid of every body |
| `measure` | `overlap a b` | the volume two bodies share |
| `measure` | `thickness` | the thinnest wall and where it is |
| `measure` | `draft` `pull=x\|y\|z` `min=` | how many faces have under `min` degrees (1 by default) of draft |

Profiles drawn inside another profile in the same sketch become holes in it.
A sketch for `sweep` is drawn around the workplane origin; `sweep` moves it to the first
path point (or the start of the helix) and turns it to face along the path.

## Workplanes

`XY` has normal +Z, `XZ` has normal -Y with y up, `YZ` has normal +X with y up.
`plane <faces>` takes flat, coplanar faces and puts the origin where the world origin
projects onto them, so sketch coordinates on a face stay world coordinates. The
plane's x and y follow "Z is up, looking at the face from outside": on side faces y is
+Z, on top faces x is +X and y is +Y, on bottom faces y is -Y. `check` prints the
origin and axes of every `plane` line.

## Selectors

Faces:

| selector | faces |
|---|---|
| `label` | every face the labelled line made |
| `label.group` | one group of them, see below |
| `>Z`, `<X`, ... | flat faces facing +Z (or -X, ...) that are furthest that way |
| `+Z`, `-X`, ... | every flat face facing +Z (or -X, ...) |
| `all` | every face |
| `a,b` | faces in `a` or `b` |

Groups each operation records:

| operation | groups |
|---|---|
| `extrude` | `start`, `end`, `side` |
| `cut` | `end` (pocket floor), `side` |
| `hole` | `bottom` (blind holes), `side`, `cbore`, `cbore_floor`, `csink` |
| `revolve` | `caps`, `side` |
| `sweep`, `loft` | `start`, `end`, `side` |
| `push` | `start`, `end`, `side` |
| `shell` | `inside`, `floor` |
| `split` | `cut` |
| `fillet`, `chamfer`, `draft`, `import` | `faces` |
| `mirror` | a copy of every group, under the mirror line's label |

A face belongs to a group while it still lies on the surface the operation made, so
groups survive later cuts and fillets that trim the face.

Edges, for `fillet` and `chamfer`:

| selector | edges |
|---|---|
| `faces` | every edge of those faces |
| `a&b` | edges where a face in `a` meets a face in `b` |
| `x\|y` | edges in `x` or `y` |

`base.end&base.side` is the top perimeter of an extrusion and leaves hole rims alone.
`fillet` and `chamfer` skip edges where the faces already meet smoothly, such as the
lines where a rounded corner of a `rect r=` joins its flat sides.
`base.side&base.side` is the vertical edges between its side walls.

## Checking a file

```
linecad check part.lcad                  # run every line, print what each did
linecad query part.lcad 'base.end&base.side' [--line N]
linecad render part.lcad part.png        # iso, top, front and right views
linecad export part.lcad part.step       # or .stl, .obj, .3mf, .svg (a four-view drawing)
linecad check part.lcad --set w=50       # override a `let` variable
linecad view part.lcad [--line N] [--select 'base.end&base.side']
```

`check` stops at the first failing line and prints its number and why. After every
line that changes the solid it prints the face count, volume and bounding box.
`view` opens a window that rebuilds on every save. The left column is the file,
one row per line, green when it ran, red where it failed. Clicking a row (or up and
down) shows the solid as it was after that line, with the faces that line made in
amber. The select box highlights whatever a selector matches at that line in cyan,
which is the quickest way to see what a `fillet` or `plane` selector will hit.

`render` lays the views out as iso (top left), top (top right), front from -Y
(bottom left) and right from +X (bottom right), each with an axis marker: X red,
Y green, Z blue.

## Limits

- Where three rounded edges meet, the corner is a sphere (or a flat triangle for a
  chamfer). Rounding at such corners
  needs flat faces, straight outside edges, and square faces wherever a rounded edge
  meets one that is not rounded.
- `draft` and pushing a face inward work on flat-sided parts, where every moved corner
  is where three flat faces meet.
- `shell` hollows the extrusion or revolve that owns the open face, so do it before
  adding other features to it. A revolve that touches its axis can only be shelled
  while it is the whole solid.
- Hole and cut edges made by booleans are fine polylines, not exact circles.
- `draft` works on profiles without holes.
