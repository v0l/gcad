# Buildings in gcad

gcad models buildings with the same lines it models parts with: one operation per line, a
`.gcad` file per building or per element family, a `.gasm` to put a building on a site or in a
larger assembly, a `.gdwg` for the drawing set. This page is the model. The operations and the
tests that prove them are in [ops.md](ops.md), under "Building datums and walls" to "Buildings
in gcad-mesh"; every row starts `missing`, and its tests in `tests/arch.rs` are written and
ignored, so each stage below is "make these tests pass and flip the rows".

## Stages

Each stage lands on master on its own, with its rows flipped, `cargo test --release` green, the
baseline (`tools/baseline.sh`, `tests/baseline/README.md`) unchanged for existing files, the
`examples/architecture/house.gasm` section for it, and `gcad render` pictures of the test house.

| stage | issue | what | depends on |
|---|---|---|---|
| A | [#348](../../issues/348) | units, building, levels, grids, layered walls, joins, hosted openings, doors, windows | |
| B | [#349](../../issues/349) | slabs, ceilings, foundations, roofs, dormers, chimneys, gutters, stairs, ramps, railings, columns, beams, joists, trusses, curtain walls, balconies | A |
| C | [#350](../../issues/350) | rooms, areas, zones, schedules, quantities, costs | A, B for slabs and ceilings |
| D | [#351](../../issues/351) | plans, reflected ceiling plans, elevations, sections, hatches, tags, automatic dimensions, sheet sets, DXF layers | A to C |
| E | [#352](../../issues/352) | IFC4 export, IFC underlay import, image underlays | A to C |
| F | [#353](../../issues/353) | terrain, pads, cut and fill, site elements, U-values, daylight, code checks, sun and shadow | A to C, D for drawing contours |
| G | [#354](../../issues/354) | the same lines in gcad-mesh for games | A and B syntax |

D, E and F can run side by side once C is in; G can start once A and B parse.

## Where the code goes

In gcad-brep, as a module family `arch/` beside `member.rs` and `enclosure.rs`
(`arch/datum.rs`, `arch/wall.rs`, `arch/opening.rs`, `arch/slab.rs`, `arch/roof.rs`,
`arch/stair.rs`, `arch/railing.rs`, `arch/structure.rs`, `arch/room.rs`, `arch/site.rs`,
`arch/check.rs`). Not a new crate:

- A wall is solids, faces, selectors and materials of `Model`; it is cut by booleans, filleted,
  measured, moved and mated like any body. All of that is `Model` in gcad-brep. A `gcad-arch`
  crate below gcad-brep could not make a body; one above it could not be called from
  `Model::apply` without an extension point and a second dispatch for every line, and it would
  still need gcad-brep's private sketch, plane and boolean code. That is the cycle the crate
  map forbids, so the code goes where its dependencies already are.
- The rule in [gcad-crates.md](gcad-crates.md) says it: a new operation of the solid language
  goes in gcad-brep. `member.rs` (structural sections) is reused by `column`, `beam`, `joist`,
  `truss` and `brace`.
- The pure parts that need no kernel go down a level so gcad-mesh (stage G) can share them:
  wall centre lines, layer offsets, join resolution, opening placement, stair rise and going,
  roof planes from a footprint, room boundaries from 2D lines. These live in a new kernel-free
  module `gcad-mesh/src/arch/` (gcad-mesh depends on gcad-parse only, and gcad-brep already
  depends on gcad-mesh), so the kernel and mesh builds agree because they run the same layout.
- Data tables go to gcad-calc beside the thread and o-ring tables: stair rules by code
  (`2R+T`, max rise, min going, headroom), door clear widths, surface resistances (Rsi, Rse),
  ISO 128 hatch names by material. Conductivities stay in `data/materials.toml` (`k`), which
  already has brick, block, concrete, PIR, mineral wool, timber, plasterboard and glass.
- Drawings (stage D) go in gcad-drafting (`drafting/plan.rs`, `drafting/hatch.rs`,
  `drafting/schedule.rs`); IFC (stage E) in gcad-drafting next to STEP (`ifc.rs`), written
  directly as ISO 10303-21 text with no new dependency; `gcad schedule` is a CLI command in
  `src/main.rs` over a `schedule` module in gcad-drafting, next to `bom.rs`.

## Units and the building header

```
units mm
building "Elm House" north=20 datum=42350
```

Lengths stay millimetres. `units` exists so a file says so, and so a later `units m` can scale
the parse; only `mm` is accepted in stage A. `building` names the project (the IFC project and
building, the title block default), `north=` is the angle in degrees from +y to true north,
counterclockwise seen from above (used by `northarrow`, the sun and IFC `TrueNorth`), and
`datum=` is the height of z=0 above the site datum in mm (level marks show both).

## Levels and grids

Levels and grids are named datums, kept in the model like `axis` and `point` and usable by
every later line.

```
level L0 0 "Ground"
level L1 2700 "First"
grid A x=0
grid B x=8000
grid 1 y=0
grid 2 y=6000
gridfile grids.csv
```

- A level name works wherever a height does: `base=L0`, `top=L1-300`, `z=L1`, `level=L1`. It is
  an expression term, so `L1+150` is a number.
- A grid crossing works wherever a 2D point does: `A1` is the point where grid `A` meets grid
  `1`; `A1+0,500` offsets it. A grid letter alone (`A`) is its line, for `dim auto grid` and
  `column_grid`. Grid names are capital letters or numbers so they do not clash with `let`
  names or labels.
- `gridfile` reads `name,axis,offset` rows.
- Levels become IFC storeys and plan and elevation level marks; grids become grid bubbles.

## Walls and wall types

A wall type is a list of layers, outside to inside, each a material from the library and a
thickness:

```
wall_type ext brick:102 air:50 pir:100 block:100 plaster:13
wall_type int block:100
```

A wall is a centre line (or outside or inside line, by `justify`) through points, built between
two heights:

```
outer: wall A1 B1 B2 A2 closed type=ext base=L0 top=L1 justify=outside
inner: wall 4000,300 4000,5700 type=int base=L0 top=L1 justify=centre
curve: wall arc 0,0 2000,2000 4000,0 type=int top=L1
shell: wall profile type=int top=L1 justify=inside
```

- Each layer is its own body, named `wall.material` (`outer.brick`, `outer.pir`; a material
  used twice is `outer.block.2`), with that material set, so mass, `gcad bom`, schedules,
  hatches and U-values read the layers directly. The wall's label is the group of them.
- `justify=outside|centre|inside` (centre unless given) says which face the drawn line is.
  Walking from point to point the outside is on the right, so a loop drawn counterclockwise
  seen from above has its outside facing out. `offset=` moves the line further, to the left.
- Layer materials come from the material library. Stage A adds four short names to
  `data/materials.toml` so wall types read as a builder writes them: `block` (dense concrete
  block, k 1.33), `plaster` (gypsum plaster, k 0.5), `tile` (ceramic tile, k 1.3) and `air`
  (an unventilated cavity: no body, a thermal resistance of 0.18 m²K/W, listed in schedules).
- `top_offset=a,b` slopes the top from `a` at the start to `b` at the end; `batter=` leans the
  wall toward its inside in degrees; `pilaster=WxD@at` adds a pier on the inside face.
- `type=ext:0..900,int:900..L1` stacks two types by height.
- Faces are selectors in the usual style: `w1.outside`, `w1.inside`, `w1.top`, `w1.bottom`,
  `w1.ends`, and per layer `w1.brick.outside`. `dim`, `label`, mates and drawings use them.
- `wall_sweep w1.inside rect 15x100 at=0` runs a profile along a face (skirting, cornice,
  coping); `wall_cut w1 <sketch>` cuts a shape through every layer.

### Joins

Walls whose ends meet are joined when the next wall line runs: an L corner mitres each layer to
the layer it meets, a T butts the stem's layers to the face of the through wall's innermost
structural layer, a cross does both. `wall_join w1 w2 mitre|butt|tee` picks one by hand.
The test is that the joined bodies neither overlap nor leave a gap: total volume equals the sum
of the plain walls minus the shared corner, and every pair of layers has zero overlap.
Walls also join to slabs (a slab at a level cuts the inner layers and stops under the outer
leaf) and trim to roofs (`roof ... walls=outer` extends and trims the walls to the roof's
underside, so gables appear).

## Hosted elements

A door, window, opening, garage door or skylight is hosted: `on=w1` names the wall (or `on=r1`
the roof), `at=` is the distance along the wall's line from its first point to the opening's
centre, `sill=` its height above the wall's base. The host keeps a list of its openings; when
the wall is built again (its points, type or base change) each opening is placed again from its
`at`, so moving the wall moves the door. The opening cuts every layer of the host, then its own
bodies are built in the hole:

```
d1: door on=outer at=2000 w=900 h=2100 swing=left hinge=left in frame=70x45 leaf=44 mark=D01
win1: window on=outer at=6000 w=1200 h=1200 sill=900 panes=2x1 frame=60 mullion=40 glazing=double head=round mark=W01
d2: door on=inner at=1500 w=826 h=2040 part=doors/flush.gcad
```

- Bodies: `d1.frame`, `d1.leaf` (`d1.leaf.2` for the second of a double, sliding or folding
  door), `d1.threshold`; `win1.frame`, `win1.glass.1`, `win1.glass.2`, `win1.mullion`. Glass is
  its own body with a glass material and opacity so the viewer, renders and GLB show it clear.
  Defaults: `frame=70x45` (face width by depth), `leaf=44`, `threshold=0` (no body), `swing=left`,
  `in`; a double glazed unit is 24 thick, triple 36. The clear width is `w` less two frame
  widths, so a 900 door is 760 clear.
- `part=` builds the door or window from a `.gcad` part with `w`, `h`, `t` (wall thickness) and
  any other variables set from the line, the way `part` does in a `.gasm`: a door family is a
  file.
- An opening past the end or top of its wall, or overlapping another opening, fails its line
  with both names.
- `mark=` is the tag in plans and the row in schedules.

## Floors, roofs, stairs and structure

Slab and roof types use the same layer list joined with `+`: `type=concrete:200+screed:50+tile:10`.
A slab on a level has its top on the level; `edge=` picks which layer sits on it. Roofs are
built from a footprint (a wall loop label or points), a type and a pitch; their ridges, hips,
valleys, eaves and verges are named edges (`r1.ridge`, `r1.eave.1`) that `gutter`,
`pitch_edge`, `dim` and drawings use. Stairs compute their riser count from the levels and the
rule: from 0 to 3000 with `max_rise=180` and `rule=2R+T=630` they have 17 risers of 176.5 and
16 treads of 277.1, reported by `gcad check`, and fail when no count satisfies the rule.
Columns, beams, joists, trusses and braces take `member` sections (`bar 200x400`, `tube`,
`channel`, `ibeam`), so the cut list and `sim frame` work on them unchanged.

## Rooms

A room is computed, not drawn: `kitchen: room at=3000,2500 name="Kitchen"` finds the smallest
closed loop of wall inner faces (and `room_separator` lines) around the point on its level,
and fails if the walls do not close. It carries name, number, level, finishes (`floor=`,
`ceiling=`, `wall=`) and computed values: net area (inside faces), gross area (to wall centre
lines or outer faces per `area_scheme`), perimeter, height (from the level to the ceiling or
the slab above) and volume. A room is not a body; it is a space in the model, drawn as a tag in
plans and written as an `IfcSpace`.

## Schedules

A schedule is a query over the model's elements, not stored data:
`gcad schedule house.gasm doors|windows|rooms|walls|slabs|roofs|stairs|columns|materials|finishes|zones|areas [--csv|--json|--svg] [--rates rates.toml]`.
Rows are sorted by name or mark and the output is byte-stable. Materials report per layer
(volume, area, length, count). Wall areas are measured on the wall's centre line, gross and
net of openings; heat loss uses outer face areas, as the U-value conventions do.
`schedule doors` in a `.gdwg` places the same table on a sheet.

## The three outputs

| element | 3D bodies | 2D (plan, section, elevation) | IFC |
|---|---|---|---|
| level | | level mark | `IfcBuildingStorey` |
| grid | | grid line and bubble | `IfcGrid` |
| wall | one body per layer | poche of cut layers, hatched by material | `IfcWall` with `IfcMaterialLayerSetUsage`, `Pset_WallCommon` |
| opening | a void in every layer | gap in the poche | `IfcOpeningElement`, `IfcRelVoidsElement` |
| door | frame, leaf, threshold | leaf line and swing arc, tag | `IfcDoor`, `IfcRelFillsElement`, `Pset_DoorCommon` |
| window | frame, glass panes, mullions | three lines in the wall, sill line, tag | `IfcWindow`, `IfcRelFillsElement`, `Pset_WindowCommon` |
| slab, ceiling, finish | one body per layer | cut hatch in sections, edge line in plans | `IfcSlab` (FLOOR, BASESLAB), `IfcCovering` |
| roof | one body per layer | roof lines in plans and elevations | `IfcRoof` with `IfcSlab` (ROOF) parts |
| stair | flights, landings, stringers | treads, arrow, UP or DN, break line | `IfcStair`, `IfcStairFlight` |
| railing | posts, balusters, rail, glass | a line | `IfcRailing` |
| column, beam | a member body | poche or dashed line | `IfcColumn`, `IfcBeam` |
| curtain wall | mullions, glass, door cells | grid line in the wall | `IfcCurtainWall` |
| room | | tag with name and area | `IfcSpace` with `IfcElementQuantity` |
| terrain, pad | a body | contours | `IfcSite` geometry |

All the bodies are ordinary bodies, so STEP, GLB, STL, `gcad render`, `measure`, mass and the
viewer work on them with no change.

## Assemblies and games

A building is a part: `part house house.gcad` in a `.gasm` brings in every layer body under
`house.`, and `move`, `rotate` and `explode` work as for any part. A staircase or a door family
can be its own `.gcad` brought in by `part`, and a site `.gasm` can hold several buildings,
the terrain and the landscape. Levels and grids of a part are visible to the assembly as
`house.L1` and `house.A1`.

For games (stage G), gcad-mesh runs the same `wall`, `wall_type`, `door`, `window`, `slab`,
`roof`, `stair`, `railing` and `column` lines with no kernel: walls are extruded footprints
with openings cut as 2D holes before extrusion, roofs are planar faces from the footprint, so
a game builds a house or a street at startup through `gcad2glb`, `gcad-bevy` and the Godot and
Unity adapters with nothing new on their side. Doors are named nodes pivoting at the hinge.
The kernel and mesh builds share the layout code in `gcad-mesh/src/arch/` and must agree on
volumes and bounds within 1 percent.

## The example: `examples/architecture/house.gasm`

Built up stage by stage, not before. A two-storey house about 10 x 8 m on a sloping site:

- `house.gasm`: `part site site.gcad`, `part house house.gcad`, `part stair stair.gcad`
  placed in the hall.
- `house.gcad`: `units mm`, `building "Example house" north=20 datum=0`, levels `L0 0`,
  `L1 2800`, `L2 5600`, `ridge`; grids `A` to `C` (x 0, 5000, 10000) and `1` to `3` (y 0,
  4000, 8000); wall types `ext` (brick 102, air 50, PIR 100, block 100, plaster 13) and `int`
  (plaster 13, block 100, plaster 13).
  - Ground floor: outer walls `A1 C1 C3 A3 closed`, internal walls making the living room
    (5000 x 4000), kitchen (5000 x 4000), hall with the stair, and a WC; front door `D01`
    900 x 2100, back door `D02` with sidelight, internal doors `D03` to `D06`, windows `W01`
    to `W06` (one a bay or a round head).
  - First floor slab `concrete:200+screed:50` with a stair opening; three bedrooms and a
    bathroom; doors `D07` to `D10`, windows `W07` to `W12`; a balcony on the back with a glass
    railing.
  - Hip roof over the outer walls, pitch 35, overhang 450, tile on battens, with a chimney
    `600 x 600` through it, gutters and two downpipes; a steel column `C1` under the balcony.
  - Rooms: living, kitchen, hall, WC, bed 1, bed 2, bed 3, bath, landing.
- `stair.gcad`: an L stair from `L0` to `L1`, width 900, with a handrail and a landing railing.
- `site.gcad`: a terrain from `site.csv` falling 1.5 m across 30 x 25 m, a pad at `L0-150`,
  a site boundary with a 3 m setback, a drive (`road`), two trees.
- `house.gdwg`, the sheet set: A-000 drawing list; A-101 ground floor plan 1:50; A-102 first
  floor plan 1:50; A-103 roof plan 1:100; A-111 ground floor reflected ceiling plan; A-201 to
  A-204 the four elevations 1:100; A-301 section A-A through the stair, A-302 section B-B
  through the living room, both 1:50; A-401 a 1:5 eaves detail viewport; A-501 door, window
  and room schedules; site plan 1:200 with contours and north arrow.

At the end it must produce, and the stage that adds each part checks it in `tests/examples.rs`:

- `gcad check house.gasm` clean, reporting stair risers and treads that satisfy the rule, the
  U-value of `ext` (about 0.17 W/m²K), each room's daylight ratio, no door under 775 mm clear.
- `gcad schedule house.gasm doors` with 10 rows `D01` to `D10`, `windows` with 12, `rooms`
  with 9 whose net areas add up to the floor areas inside the walls, `materials` per layer,
  and `walls` net of openings.
- `gcad export house.gasm house.ifc`: 3 storeys, every wall, door, window, slab, roof, stair,
  railing, column and space present and contained in a storey, every opening voiding a wall
  and filled by its door or window, layer sets with the `ext` and `int` thicknesses, stable
  GlobalIds between two runs, and (where IfcOpenShell is installed) `ifcopenshell.validate`
  clean with element volumes within 0.5 percent of gcad's.
- `gcad export house.gdwg house.pdf` with the sheets above, and `house.dxf` with `A-WALL`,
  `A-DOOR`, `A-GLAZ`, `A-GRID` layers.
- `gcad render house.gasm house.png` and `gcad2glb house.gasm house.glb`; the mesh-only build
  of `house.gcad` under 4000 triangles.
- `gcad export site.gcad` reporting the pad's cut and fill volumes.
