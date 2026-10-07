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

The file runs against five pieces of state, the way G-code runs against a machine:

| state | set by | used by |
|---|---|---|
| variables | `let` | every expression |
| workplane | `plane` (XY until set) | sketch operations, `extrude`, `cut`, `revolve`, `hole` |
| sketch | `rect`, `circle`, `poly` | `extrude`, `cut`, `revolve`, `sweep`, which empty it |
| path | `path` | `sweep` |
| solid | every operation that makes or removes material | `fillet`, `chamfer`, selectors |

## Operations

| operation | parameters | does |
|---|---|---|
| `let` | `name=expr ...` | sets variables, in order, so later pairs can use earlier ones |
| `plane` | `on` `offset=` | sets the workplane to `XY`, `XZ`, `YZ` or a flat face selector, moved `offset` along its normal |
| `rect` | `w h` `at=x,y` `r=` | adds a rectangle centred on `at`, corners rounded by `r` |
| `circle` | `d` `at=x,y` | adds a circle |
| `poly` | `x,y x,y x,y ...` | adds a closed polygon |
| `extrude` | `d` `draft=` | adds the sketch as material along the plane normal (negative `d` goes the other way), tapered inward by `draft` degrees |
| `cut` | `d` or `thru`, `draft=` | removes the sketch from the solid, going into it against the plane normal |
| `revolve` | `angle` `axis=x\|y` | spins the sketch about the workplane's x or y axis through its origin and adds it |
| `path` | `x,y,z x,y,z ...` `r=` | sets the sweep path in world coordinates; corners are bent with radius `r` |
| `sweep` | | carries the sketch from the path start along the path and adds it |
| `hole` | `d x,y ...` `depth=` | drills holes at each point on the workplane, through unless `depth` is given |
| `fillet` | `size edges` | rounds the selected edges with radius `size` |
| `chamfer` | `size edges` | bevels the selected edges by `size` |

Profiles drawn inside another profile in the same sketch become holes in it.
A sketch for `sweep` is drawn around the workplane origin; `sweep` moves it to the first
path point and turns it to face along the first segment.

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
| `hole` | `bottom` (blind holes), `side` |
| `revolve` | `caps`, `side` |
| `sweep` | `start`, `end`, `side` |
| `fillet`, `chamfer` | `faces` |

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

- Fillets and chamfers fail where three selected edges meet at one corner, such as
  every edge of a box. Round the vertical edges in the sketch with `rect r=` instead.
- Hole and cut edges made by booleans are fine polylines, not exact circles.
- `draft` works on profiles without holes.
