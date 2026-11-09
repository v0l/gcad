# Architecture

Buildings are a `.gcad` file in millimetres: datums first (units, levels, grids), then walls, then
everything hosted on them. Lines refer to levels and grid points by name, so a change to a level
height moves every wall, slab and roof that uses it. Full syntax per op is in `docs/ops.md`
("Building datums and walls" to "Building structure and facade"); a whole house is
`examples/architecture/house.gcad`, wrapped by `house.gasm`.

Contents: datums, walls, openings, slabs and ceilings, roofs, stairs and railings, structure and
facade, not yet built.

```gcad
units mm
building "Cabin" north=0 datum=0
level L0 0 "Ground"
level L1 2700 "Roof"
grid A x=0
grid B x=6000
grid 1 y=0
grid 2 y=4000
wall_type ext brick:102 pir:100 block:100
outer: wall A1 B1 B2 A2 closed type=ext base=L0 top=L1 justify=outside
D01: door on=outer at=1500 w=900 h=2100 swing=left mark=D01
W01: window on=outer at=7000 w=1200 h=1200 sill=900 panes=2x1 mark=W01
floor: slab L0 A1 B1 B2 A2 type=concrete:200
roof1: roof outer type=gable pitch=35 overhang=400 thickness=200 ridge=x base=L1 walls=outer
```

## Datums

- `units mm`, `building "Name" north=20 datum=0`.
- `level L1 2700 "First"`, then `base=L0 top=L1-300` anywhere a height goes.
- `grid A x=0`, `grid 1 y=0`; `A1` and `A1+0,500` work anywhere a point goes. `gridfile grids.csv`
  loads them.

## Walls

- `wall_type ext brick:100 pir:100 block:100` lists layers outside to inside, materials from the
  library. Each layer is its own body, `w1.brick`.
- `w1: wall x,y x,y type=ext base=L0 top=L1 justify=outside|centre|inside offset=-150`. Several
  points make a polyline, `wall arc x,y x,y x,y` an arc, `closed` a loop, and `rect ...` then
  `wall profile type=` a wall from a sketch.
- Shape: `top_offset=0,600` and `batter=1` slope it, `pilaster=300x200@2000` adds pilasters,
  `type=ext:0..900,int:900..L1` stacks types. Walls that meet join themselves; override with
  `wall_join w1 w2 mitre|butt|tee`.
- `wall_sweep w1.inside rect 15x100 at=0` runs skirting or coping along a wall, `wall_cut w1 rect
  1000 500 at=2000,2450` cuts a profile through it.
- Faces select as `w1.outside`, `w1.inside`, `w1.top`, `w1.ends`.

## Openings

All take `on=wall at=` (distance along the wall) and move with it. One past the wall's end or top,
or over another, fails.

- `o1: opening on=w1 at=1200 w=900 h=2100 sill=0`
- `d1: door on=w1 at=1200 w=900 h=2100 hinge=left in|out swing=left|right|double|slide|fold
  frame=70x45 mark=D01`; bodies `d1.frame`, `d1.leaf`.
- `win1: window on=w1 at=3000 w=1200 h=1200 sill=900 panes=2x1 head=flat|arch|segment|round
  glazing=double|triple mark=W01`.
- `garage_door on=w1 at= w= h= panels=4`; `door on=w1 ... part=flush.gcad` takes a part file.

## Slabs, ceilings and ground

- `s1: slab L1 A1 B1 B2 A2 type=concrete:200+screed:50` (the top of the last layer sits on the
  level), `slope=0.02 falls_to=x,y`, `slab_opening s1 x,y ...`.
- `room hall at=2000,3000` first names a rectangular room, then `c1: ceiling room=hall h=2400
  type=gypsum:13` and `f1: floor_finish room=hall type=oak:20` fill it.
- `foundation strip outer w=600 d=900`, `foundation pad x,y w= l= d=`, `ground_slab outer
  type=concrete:150+pir:100`.

## Roofs

- `r1: roof outer type=gable pitch=45 overhang=450 thickness=200 ridge=x base=L1`. Types: `hip`,
  `half_hip`, `shed`, `flat`, `mansard`, `gambrel`, `barrel`. `walls=outer` trims walls to the
  underside.
- `pitch_edge r1.eave.1 60` per edge, `roof_join r2 r1`, and `roof extrude 3000 thickness=200`
  after a profile. Edges select as `r1.ridge`, `.hip`, `.valley`, `.eave`, `.verge`.
- `dormer on=r1 at=x,y w=1200 type=gable`, `skylight on=r1 at=x,y w=780 h=980`, `chimney x,y w=600
  d=600 up_to=r1 over=900`, `gutter along=r1.eave.1 w=100 slope=0.005`, `downpipe from=gu.end d=68
  to=L0`, `parapet outer h=600 coping=50`.

## Stairs and railings

- `st1: stair from=L0 to=L1 at=x,y dir=0 width=1000 rise=auto tread=auto rule=2R+T=630
  max_rise=180 shape=straight|L|U|winder|spiral landing=900 stringer=closed|open|mono nosing=20`.
  A stair too steep for the rule or under 2000 headroom fails. `stair_cut st1 s1 headroom=2000`
  opens the slab above.
- `ramp from=x,y,z to=x,y,z slope=1:12 width=1200 landing=1500 every=10000`, `ladder x,y,z x,y,z
  width=400 rungs=@300`.
- `railing path x,y,z ... height=900 post=50@1500 baluster=12@120 top=round:40 material=steel`,
  `railing along=st1.edge`, `railing ... glass=12 post=none`, `handrail path ... d=40
  bracket=@1000`.

## Structure and facade

- `c1: column A1 rect 300x300|round 300 base=L0 top=L1 material=`, `column_grid rect 200x200
  base=L0 top=L1`.
- `b1: beam A1 B1 bar 200x400 z=L1 material=c24` (top of beam on `z`), `joist <outline> bar 47x200
  spacing=400 dir=y z=L1`, `truss x,y,z x,y,z type=king|queen|fink pitch=30 member=bar 47x150
  spacing=600 count=10`.
- `curtainwall x,y x,y height=L1 grid=1500x900 mullion=50x100 glass=double door=cell:2,1`,
  `louvre x,y x,y z=900..2100 blade=150x20@100 angle=45`, `shading_fin x,y x,y depth=400 t=20@500`,
  `balcony on=outer at=4000 level=L1 w=3000 d=1200 slab=concrete:200 railing=1100`.

Check structure with `sim frame` (see [simulation](simulation.md)) and wall U-values with `sim
conduction`.

## Not yet built

Rooms with areas and zones, schedules, architectural drawings, IFC and site and analysis ops are
rows marked `missing` in `docs/ops.md`. Do not write them; run `gcad docs` to see what exists.
