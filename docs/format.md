# The gcad format

A `.gcad` file is a list of operations, one per line, run from top to bottom. Every
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
- Numbers are expressions: `+ - * /`, parentheses, `pi`, names set by `let`, and the
  functions `sqrt abs sin cos tan asin acos atan atan2 hypot min max floor ceil round`,
  like `atan2(y,x)`. Angles are in degrees.
  Expressions cannot contain spaces.
- A 2D point is `x,y`, a 3D point is `x,y,z`, each part an expression.
- Lengths are millimetres, angles are degrees.

## State

The file runs against these pieces of state, the way G-code runs against a machine:

| state | set by | used by |
|---|---|---|
| variables | `let` | every expression |
| workplane | `plane` (XY until set) | sketch operations, `extrude`, `cut`, `revolve`, `hole`, `helix`, `repeat` |
| sketch | `rect`, `circle`, `poly`, `ngon`, `gear`, `slot`, `ellipse`, `spline`, `text`, `pen` ... `close` | `extrude`, `cut`, `revolve`, `sweep`, `section`, which empty it |
| sections | `section` | `loft` |
| path | `path`, `helix` | `sweep`, `plane path`, `repeat along=path` |
| points | `point`, `dist`, `horizontal`, `vertical`, `angle` | any 2D point |
| axes | `axis` | `revolve`, `rotate` |
| solid | every operation that makes or removes material | `fillet`, `chamfer`, `shell`, `draft`, `push`, selectors |
| bodies | `body` | `combine`, and export, render and assemblies, which take every body |

## Operations

Variables, conditions and other files:

| operation | parameters | does |
|---|---|---|
| `let` | `name=expr ...` | sets variables, in order, so later pairs can use earlier ones; a variable given with `--set` keeps its outside value |
| `if` | `condition operation ...` | runs the rest of the line only when the condition is not zero; `< > <= >= == !=` give 1 or 0 |
| `include` | `file.gcad` `name=value ...` | runs another file's lines here, with those variables fixed; the file is relative to this one |

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
| `gear` | `teeth module` `internal` `at=x,y` `angle=` `pressure=` `backlash=` | adds an involute spur gear with a tooth on `angle` (0 by default, along x), teeth of `module` (pitch diameter `teeth*module`), a `pressure` angle of 20 degrees unless given, and teeth thinned by `backlash` at the pitch circle. `internal` draws the toothed hole of a ring gear instead, to go inside a larger profile, with its tooth on `angle` pointing in. Two gears mesh at centres `module*(teeth_a+teeth_b)/2` apart, a gear inside a ring at `module*(ring-gear)/2`, with a tooth of one pointing at a gap of the other |
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
| `thread` | `size` `on=faces` `pitch=` `left` | cuts an ISO metric thread (`M2` to `M12`) into a round face: a rod gets an outside thread with its crest on the rod, a hole an inside one with its crest on the drill; `pitch` overrides the coarse pitch and `left` makes it left-hand. Each end of the face must be a flat face square to it or a chamfer (or countersink) that goes past the thread's root, which the thread runs out into. Chamfer before threading; a rounded end, or a chamfer after, fails |
| `shell` | `t` `open=faces,...` | hollows an extrusion (open `label.end`, `label.start` or both) or a full revolve (open its flat `label.caps`) to walls `t` thick; where `fillet` rounded the extrusion's straight edges, the inside is rounded to that radius less `t`, so the walls stay `t` thick |
| `draft` | `angle faces` `neutral=face` | tilts flat side faces inward by `angle` degrees, hinged where they meet the neutral face; `label.side` of an extrusion that is the whole solid tapers curved sides too |
| `push` | `faces d` | moves flat faces `d` along their normal, out (positive) or in (negative); an extrusion's `label.end` or `label.start` moves in even with curved sides |
| `rib` | `t` | after an open `pen` path of one `line`, fills between that line and the solid with a web `t` thick, centred on the workplane |
| `wrap` | `faces depth=` `raise` | rolls the sketch onto a round face and cuts it `depth` in, or with `raise` stands it `depth` out. Draw the sketch on a plane along the cylinder's axis: its x runs around the cylinder and its y along the axis, read from the side the plane faces |
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
| `body` | `name` | sets the current solid aside and starts a new one: another part sharing this file's sizes, which an assembly brings in as `part.name`, or a piece to `combine` into one part; the first body is `main` |
| `combine` | `into from` `mode=add\|cut\|intersect` | joins body `from` into body `into`, or cuts it out, and drops `from` |
| `place` | `body` `on=faces` | moves the body so its lowest point sits on the faces |
| `import` | `file.step` or `file.stl`, `solid=` | adds the solid in a STEP file, or a watertight STL mesh with its flat regions merged into faces, relative to the `.gcad` file. A STEP file with several solids needs `solid=n` to pick one; the error lists them with their names and sizes |
| `color` | name, `#rrggbb` or `r,g,b` | colours the current body in STEP and 3MF files |
| `material` | `name` `density=` | sets what the current body is made of, for its mass. Known names: steel, stainless, aluminium, brass, copper, titanium, pla, petg, abs, asa, nylon, tpu, polycarbonate, acrylic, resin, wood. `density=` in g/cm³ gives any other |

Measuring, which changes nothing and prints the answer:

| operation | parameters | prints |
|---|---|---|
| `measure` | `faces faces` | the distance between two sets of faces |
| `measure` | `mass` | volume, surface area and centroid of every body, and the mass in grams of those with a `material` |
| `measure` | `overlap a b` | the volume two bodies share |
| `measure` | `thickness` | the thinnest wall and where it is |
| `measure` | `draft` `pull=x\|y\|z` `min=` | how many faces have under `min` degrees (1 by default) of draft |

Profiles drawn inside another profile in the same sketch become holes in it.
A sketch for `sweep` is drawn around the workplane origin; `sweep` moves it to the first
path point (or the start of the helix) and turns it to face along the path.

## Assemblies

A design with more than one part is a `.gasm` file, an assembly: it brings the parts
in from part files, places them and joins them, and makes no geometry of its own. A
`.gcad` part file holds one part, or several parts as bodies when they share sizes.
Its lines have the same shape and use `let`, `if` and `include` the same way.

| operation | parameters | does |
|---|---|---|
| `part` | `name file.gcad` `body=` `variable=value ...` | runs a part file with those variables fixed and brings in its bodies; one body is called `name`, several are `name.body`; `body=` takes just that one. A `.gasm` file brings in its parts and joints too |
| `move` | `part x,y,z` | moves a part and everything jointed to it |
| `rotate` | `part angle` `axis=x\|y\|z\|name` `about=x,y,z` | turns a part and everything jointed to it |
| `axis` | `name x,y,z x,y,z` | names a line for joints and `rotate` |
| `joint` | `name child parent turn about=axis` or `slide along=x,y,z`, `min=` `max=` `at=` | joins two parts; a `turn` joint spins the child about a datum `axis` by degrees, a `slide` joint moves it along a direction by millimetres; `at` poses it now |
| `pose` | `joint value` | moves a joint to a new value |
| `couple` | `driven driver` `ratio=` | makes one joint follow another: `driven` moves `ratio` of its units (degrees or millimetres) for each unit `driver` moves, from where both are now. Gears turn at `ratio=-teeth_a/teeth_b`; a rack slides `ratio=pi*d/360` mm per degree of its pinion. A driven joint cannot be posed itself |
| `concentric` | `moving:faces fixed:faces` `near=x,y,z` `flip` | turns and moves the moving part so its round faces share an axis with a hole in the fixed one; `near` picks the hole nearest a point, otherwise the one nearest where the part is; it can still slide along and spin about that axis |
| `flush` | `moving:faces fixed:faces` `offset=` | turns and moves the moving part so its flat faces lie on the fixed part's, facing each other, `offset` apart |
| `parallel` | `moving:faces fixed:faces` | turns the moving part so its face or axis is parallel to the fixed one; an axis and a face are parallel when the axis lies along the face |
| `angle` | `moving:faces fixed:faces degrees` | turns the moving part so its face or axis is at that angle to the fixed one; between two faces it is the angle between their outward normals, so 0 faces the same way and 180 faces each other |
| `distance` | `moving:faces fixed:faces length` | makes two faces, two axes, or an axis and a face parallel and `length` apart, keeping the moving part on the side it is on |
| `tangent` | `moving:faces fixed:faces` | lays a round face on a flat one, or against another round face, with its axis parallel |
| `aligned` | `a:faces b:faces` `tol=` | checks that every hole in `a` has a parallel hole in `b` on the same axis, within `tol` (0.05 by default), and fails the line if not |
| `pattern` | `part holes=part:faces`, or `count=` with `step=x,y,z` or `angle=` `axis=` | copies a part. `holes=` puts a copy in each other hole of a set the part is `concentric` with, and gives each copy the part's mates; `step=` and `angle=` space `count` copies out from the part. Each copy also gets the part's joints, named `joint_2` and so on, with their couples, so the planets of a planetary gear are one part and a pattern. Copies are called `part_2`, `part_3` and so on |
| `explode` | `part x,y,z` | moves a part and the parts held by it by that much in exploded views only: `render --explode` and the viewer's explode slider. It does not change where the part is for mates, joints, interference or export |
| `color` | `part colour` | colours a part, over the colour its file gave it |
| `material` | `part name` `density=` | sets a part's material, over the one its file gave it |
| `interference` | `none`, `joint=` `steps=` | reports the volume each pair of parts shares; `none` fails the line if any do; `joint=` checks across the joint's range in `steps` |
| `measure` | `mass`, `overlap a b` | as in part files |

`faces` in `parallel`, `angle`, `distance` and `tangent` is either flat faces on one
plane or the round faces of one cylinder, which stands for its axis.

`examples/robot/arm.gasm` puts these together. Each joint is a planetary reducer: the
motor is mated to the housing, its sun turns on it with a `turn` joint coupled to the
arm joint at `1+ring/sun`, and one planet turns on the output link coupled at
`-ring/planet`; `pattern planet count=3 angle=360 axis=...` adds the other two with
their joints and couples. The gripper's fingers are racks on one pinion, so one `slide`
drives both. The arm's own variables pose it:
`gcad check examples/robot/arm.gasm --set shoulder=30 --set jaw=6`.

A part can have several joints to the same parent, and their motions add up. They
must not depend on the order they are applied in, so they are slides in any
direction, or turns about one axis together with slides along it. A turn and a slide
on the same axis make a cylindrical joint:

```
axis shaft 0,0,0 0,0,1
joint spin rod frame turn about=shaft
joint push rod frame slide along=0,0,1 min=0 max=20
```

A `turn` joint without `min=` or `max=` turns all the way round: posing it to 270
leaves it at -90.

When a mate joins two parts that already hang off different joints, it closes a
loop, like the coupler and rocker of a four-bar linkage. Moving one joint in the loop
moves the others with it so the mate keeps holding, if they can; otherwise the move
fails as before. `pose` says which joints followed. See
`examples/assemblies/linkage.gasm`.

A mate holds the parts together, the way a joint does. The first mate of a part
places it and fixes it to the other part, so it moves when that part moves. Later
mates of the same part to other parts do not move it: they must already hold, and
from then on they keep holding. A joint that would pull a mate apart cannot move:
`pose` fails and the viewer locks its slider. A screw mated to a lid's hole and a
box's pilot hole keeps a hinged lid shut. `aligned` only checks holes; it holds
nothing.

`part:faces` names faces on one part with the selectors of its own file, so
`case.main:pilot.side` is the side of the `pilot` holes in the part `case.main`. They
follow the part as it moves.

```
let open=0 screwed=1 bx=32 by=17 h=30
part case ../parts/enclosure.gcad
axis hinge -40,25,30 40,25,30
joint lid case.lid case.main turn about=hinge min=-110 max=0 at=open
aligned case.lid:screws.side case.main:pilot.side
if screwed part s1 ../parts/screw.gcad
if screwed concentric s1:shank.side case.lid:screws.side near=bx,by,h
if screwed flush s1:head.start case.lid:top.end
if screwed concentric s1:shank.side case.main:pilot.side
if screwed explode s1 0,0,25
if screwed pattern s1 holes=case.lid:screws.side
pose lid open
explode case.lid 0,0,30
interference none
```

`gcad bom file` lists what a file is made of: one row per body of each part file
and set of variables, with how many there are, the material, and the volume and mass
of each. `--csv` writes the same as CSV, with the part names in the last column.

A `.step` export of an assembly keeps its structure: each part file and set of
variables becomes one product, and each part placed in the assembly is an instance of
it with its own name and position, so four screws are one screw used four times.

An `.svg` export of an assembly draws it exploded, if it has `explode` lines, with a
parts list (item, quantity, part, material, mass) and a numbered balloon on one of
each item in the iso view.

`check`, `render`, `export` and the viewer take either kind of file, and `--set`
reaches the assembly's own variables. The viewer rebuilds when any part file next to
the assembly is saved.

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
| `fillet`, `chamfer`, `draft`, `thread`, `import` | `faces` |
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
gcad check part.gcad                  # run every line, print what each did
gcad query part.gcad 'base.end&base.side' [--line N]
gcad render part.gcad part.png        # iso, top, front and right views
gcad export part.gcad part.step       # or .stl, .obj, .3mf, .svg (a four-view drawing)
gcad export part.gcad part.svg --section y=0   # adds a hatched section across y=0
gcad check part.gcad --set w=50       # override a `let` variable
gcad check part.gcad --time           # also show how long each line took
gcad docs                             # print this reference
gcad part.gcad                        # open the viewer; plain `gcad` starts with a file picker
gcad view part.gcad [--line N] [--select 'base.end&base.side']
```

An `.svg` drawing shows the top, front, right and iso views with the overall width,
depth and height dimensioned, and calls out round holes in the views that look down
them, like `4× ⌀3.2`. `--section x=`, `y=` or `z=` adds a view of the part cut
across that plane, looking at the cut, with the cut faces hatched.

`check` stops at the first failing line and prints its number and why. After every
line that changes the solid it prints the face count, volume and bounding box.
`view` opens a window that rebuilds on every save. The left column is the file,
one row per line, green when it ran, red where it failed. Clicking a row (or up and
down) shows the solid as it was after that line, with the faces that line made in
amber. The select box highlights whatever a selector matches at that line in cyan,
which is the quickest way to see what a `fillet` or `plane` selector will hit.

The viewer also has:

- a view cube in the corner: click a face of it, or the iso, top, front, right, back,
  left and bottom buttons under it, to look from that side;
- measure mode (the measure button): click two points on the model to get the
  distance between them and along each axis, corners snap, and each pick names the
  face it landed on with its area;
- section: cuts the model across x, y or z at a slider position, with flip to show
  the other half; the cut faces are orange, and measure picks ignore what is cut away;
- ortho: on by default, as in most mechanical CAD; switch it off for a perspective view;
- a parts card listing every body in its `color`, with a switch to hide each one, and
  an explode slider when the assembly has `explode` lines;
- a joints card with a slider for every `joint`, which moves the parts without
  rebuilding, follows couples and closed loops, locks joints that mates hold, and
  checks for parts that overlap at the slider positions;
- open (or ctrl-O) to switch to another `.gcad` file.

`render` lays the views out as iso (top left), top (top right), front from -Y
(bottom left) and right from +X (bottom right), each with an axis marker: X red,
Y green, Z blue.

## Limits

- Where three rounded edges meet, the corner is a sphere (or a flat triangle for a
  chamfer). Where an inside edge meets two outside ones, or an outside edge two inside
  ones, as at the rim of a pocket or the foot of a boss, it is a torus patch (or a flat
  quad). Rounding at such corners needs flat faces, straight edges, the face across the
  odd edge square to its sides, and square faces wherever a rounded edge meets one that
  is not rounded. An inside and an outside rounded edge cannot meet at a sharp one.
- `draft` and pushing a face inward work on flat-sided parts, where every moved corner
  is where three flat faces meet.
- `shell` hollows the extrusion or revolve that owns the open face, so do it before
  adding other features to it. A revolve that touches its axis can only be shelled
  while it is the whole solid.
- Edges a boolean makes are exact lines and circles where they are flat and round, as at
  the rim of a hole; other intersections, such as two crossing cylinders, are fine polylines.
- `draft` works on profiles without holes.
