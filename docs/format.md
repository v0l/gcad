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
| path | `path`, `helix` | `sweep` |
| solid | every operation that makes or removes material | `fillet`, `chamfer`, `shell`, `draft`, `push`, selectors |
| bodies | `body` | export and render, which take every body |

## Operations

Sketch:

| operation | parameters | does |
|---|---|---|
| `let` | `name=expr ...` | sets variables, in order, so later pairs can use earlier ones |
| `plane` | `on` `offset=` `rx=` `ry=` `rz=` | sets the workplane to `XY`, `XZ`, `YZ` or a flat face selector, moved `offset` along its normal, then turned about its own x, y and normal axes by degrees |
| `rect` | `w h` `at=x,y` `r=` | adds a rectangle centred on `at`, corners rounded by `r` |
| `circle` | `d` `at=x,y` | adds a circle |
| `ellipse` | `dx dy` `at=x,y` | adds an ellipse of those diameters |
| `poly` | `x,y x,y x,y ...` | adds a closed polygon |
| `ngon` | `d n` `at=x,y` `angle=` | adds a regular polygon of `n` sides with corners on a circle of diameter `d` |
| `slot` | `l w` `at=x,y` `angle=` | adds a slot `l` long overall and `w` wide, turned by `angle` degrees |
| `spline` | `x,y x,y ... closed` | adds a smooth closed curve through the points |
| `text` | `"words"` `size=` `at=x,y` | adds the outlines of the words, `size` tall, starting at `at` |
| `pen` | `x,y` | starts a path of lines and arcs at a point |
| `line` | `x,y` | draws a straight segment to the point |
| `arc` | `x,y` `via=x,y` | draws an arc to the point through `via` |
| `close` | | closes the path back to its start and adds it to the sketch |

Features:

| operation | parameters | does |
|---|---|---|
| `extrude` | `d` or `upto=faces`, `both`, `draft=`, `mode=add\|cut\|intersect` | adds the sketch along the plane normal (negative `d` goes the other way); `both` centres it on the plane; `upto` stops at a flat face; `draft` tapers it inward by degrees; `mode=intersect` keeps only what the solid and the extrusion share |
| `cut` | `d` or `thru`, `draft=` | removes the sketch from the solid, going into it against the plane normal |
| `revolve` | `angle` `axis=x\|y` | spins the sketch about the workplane's x or y axis through its origin and adds it |
| `path` | `x,y,z x,y,z ...` `r=` | sets the sweep path in world coordinates; corners are bent with radius `r` |
| `helix` | `r= pitch= turns=` `at=x,y` | sets a helical sweep path about the workplane normal |
| `sweep` | | carries the sketch from the path start along the path and adds it |
| `section` | | stores the sketch as one cross-section for `loft` |
| `loft` | | joins the stored sections with ruled faces and adds the result; every section needs the same number of edges |
| `hole` | `d x,y ...` `depth=` `cbore=d,depth` `csink=d,angle` `thread=M4` | drills holes at each point, through unless `depth` is given, with an optional counterbore or countersink; `thread` checks the drill suits the tap and records it |
| `shell` | `t` `open=label.end` | hollows an extrusion to walls `t` thick, leaving the named cap open |
| `draft` | `angle faces` `neutral=face` | tilts flat side faces inward by `angle` degrees, hinged where they meet the neutral face |
| `push` | `faces d` | moves flat faces `d` along their normal, out (positive) or in (negative) |
| `fillet` | `size edges` `to=` | rounds the edges with radius `size`, or from `size` to `to` along them |
| `chamfer` | `size edges` `d2=` | bevels the edges by `size`; with `d2`, the edges must be written `a&b` and `size` is cut along `a`, `d2` along `b` |

Bodies:

| operation | parameters | does |
|---|---|---|
| `mirror` | `on` `offset=` | adds the mirror image of the solid across a named plane or flat face |
| `repeat` | `label` `count=` and `step=x,y` or `angle=` | repeats what a labelled line added or removed, in a row along the workplane or around its normal |
| `move` | `x,y,z` | moves the solid |
| `rotate` | `angle` `axis=x\|y\|z` `about=x,y,z` | turns the solid |
| `scale` | `factor` `about=x,y,z` | scales the solid |
| `split` | `on` `offset=` `keep=below\|above` | cuts the solid with a plane and keeps one side |
| `body` | `name` | sets the current solid aside and starts a new one |
| `import` | `file.step` | adds the solids in a STEP file, relative to the `.lcad` file |

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
`base.side&base.side` is the vertical edges between its side walls.

## Checking a file

```
linecad check part.lcad                  # run every line, print what each did
linecad query part.lcad 'base.end&base.side' [--line N]
linecad render part.lcad part.png        # iso, top, front and right views
linecad export part.lcad part.step       # or .stl
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

- Where three rounded edges meet, the corner is a sphere. Rounding at such corners
  needs flat faces, straight outside edges, and square faces wherever a rounded edge
  meets one that is not rounded.
- `draft` and pushing a face inward work on flat-sided parts, where every moved corner
  is where three flat faces meet.
- `shell` hollows the extrusion that owns the open face, so do it before adding other
  features to that extrusion.
- Hole and cut edges made by booleans are fine polylines, not exact circles.
- `draft` works on profiles without holes.
