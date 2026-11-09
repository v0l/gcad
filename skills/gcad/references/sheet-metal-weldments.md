# Sheet metal and weldments

## Sheet metal

`sheet t r= k=`, then a rect or poly and `base: tab` for the flat base, then
`walls: flange base.end&>X 10` for each bent edge (`angle=`, off a top edge it rises up, off a
`base.start` edge it goes down). Flange a flange from its `end`; pick the one edge on the side
the new flange should rise from (`>X&walls.end`, `walls.face&walls.end`), since selecting both
sides of an edge fails. Put holes and through cuts in the plates so they reach the flat
pattern, then `gcad export part.gcad flat.dxf`, or end the file with `unfold` to see it.

A laser-cut and bent part is drawn flat first: sketch the whole blank (any outline, `slot` for
round ends, holes as circles), `blank: tab`, cut the holes and keyways while it is flat, then
`left: bend a,b c,d angle=-90` along each bend line in the tab's workplane. The side with the
sketch origin stays put; labels made on the blank carry over, and the blank is the flat
pattern. The blank must cross each bend on straight edges with no holes in the bending strip.
`dart left -7 7 l=20 w=8` presses stiffening darts into a bend; keep them clear of holes.

```text
sheet 2 r=2 k=0.4
rect 60 40
base: tab
walls: flange base.end&>X 10
unfold
```

The block above is a fragment: run `gcad query` for the edge names on your own part.

## Weldments

`member tube 40x40x3 0,0,0 500,0,0 500,300,0 0,300,0 closed` lays a mitred frame of square
tube, one body per straight run; also `bar`, `pipe DxT`, `rod D`, `angle`, `channel`.
`gcad bom` turns them into a cut list. `up=`, `rotate=` and `offset=u,v` set how the profile
sits on the path. `material=wood` assigns every member. Add `count=19 step=400,0,-10`
for equally spaced copies in one line, or `count=8 angle=360 axis=hub` for a circular array.
The count includes the seed; copies retain the material, finish and cut-list entry. In a row,
`end_step=x,y,z` moves only the last point (studs under a sloping roof) and `when=expr` skips
instances where it is 0, with `k` the index (openings). Never write one `member` line per stud.

```gcad
member tube 40x40x3 0,0,0 500,0,0 500,300,0 0,300,0 closed
```
