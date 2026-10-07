# Operations and their tests

Every operation a parametric CAD program is expected to have, whether linecad has it,
the line that does it, and the tests that prove it. Each test builds a part and checks
it against a volume or bounding box worked out by hand.

A `missing` row's tests are already written with the syntax the operation should take
and are marked `#[ignore = "missing: ..."]`. Building the operation means making those
tests pass and flipping the row to `supported`. `tests/ops_doc.rs` fails if this table
and the tests disagree: a test not listed here, a listed test that does not exist, a
`supported` row whose test is ignored as missing, or a `missing` row whose test runs.

Run everything with `cargo test --release`. The OpenCascade check needs a Python with
OCP: `LINECAD_OCP_PYTHON=~/git/cadkit/.venv/bin/python cargo test --release -- --ignored step_opens`.

## Sketch

| operation | status | syntax | tests |
|---|---|---|---|
| rectangle | supported | `rect 30 20 at=10,-5` | `sketch::rect`, `sketch::rect_offset_centre` |
| rounded rectangle | supported | `rect 30 20 r=4` | `sketch::rounded_rect` |
| circle | supported | `circle 10 at=x,y` | `sketch::circle` |
| polygon | supported | `poly 0,0 30,0 0,20` | `sketch::polygon`, `sketch::concave_polygon` |
| holes in a profile | supported | a profile drawn inside another | `sketch::profile_inside_profile_is_a_hole` |
| several profiles | supported | profiles side by side | `sketch::separate_profiles` |
| regular polygon | missing | `ngon 20 6` | `sketch::regular_polygon` |
| slot | missing | `slot 30 10` | `sketch::slot` |
| ellipse | missing | `ellipse 20 10` | `sketch::ellipse` |
| lines and arcs | missing | `pen`, `line`, `arc via=`, `close` | `sketch::lines_and_arcs` |
| spline | missing | `spline x,y ... closed` | `sketch::spline` |
| text | missing | `text LINECAD size=10` | `sketch::text` |

## Workplanes

| operation | status | syntax | tests |
|---|---|---|---|
| XY, XZ, YZ | supported | `plane XZ` | `workplane::xy`, `workplane::xz`, `workplane::yz` |
| offset plane | supported | `plane XY offset=10` | `workplane::offset` |
| plane on a face | supported | `plane base.end` | `workplane::on_a_face`, `workplane::side_face_keeps_world_coordinates` |
| rotated plane | missing | `plane XY rx=90` | `workplane::rotated` |

## Features

| operation | status | syntax | tests |
|---|---|---|---|
| extrude | supported | `extrude 5`, `extrude -5` | `features::extrude`, `features::extrude_reversed` |
| extrude with draft | supported | `extrude 10 draft=5` | `features::extrude_draft` |
| symmetric extrude | missing | `extrude 10 both` | `features::extrude_symmetric` |
| extrude up to a face | missing | `extrude upto=base.end` | `features::extrude_up_to_face` |
| cut | supported | `cut 4`, `cut thru` | `features::cut_blind`, `features::cut_through` |
| cut with draft | supported | `cut 4 draft=3` | `features::cut_draft` |
| revolve | supported | `revolve 360 axis=y`, `revolve 90` | `features::revolve_full`, `features::revolve_partial` |
| sweep along a path | supported | `path x,y,z ... r=8` then `sweep` | `features::sweep_bent_path`, `features::sweep_straight_path` |
| sweep along a helix | missing | `helix r=10 pitch=5 turns=3` then `sweep` | `features::sweep_helix` |
| loft | missing | `section` per profile, then `loft` | `features::loft` |
| shell | missing | `shell 2 open=base.end` | `features::shell` |
| face draft | missing | `draft 5 base.side neutral=base.start` | `features::face_draft` |
| push or pull a face | missing | `push base.end 2` | `features::push_face` |

## Holes

| operation | status | syntax | tests |
|---|---|---|---|
| through hole | supported | `hole 4 x,y ...` | `holes::through`, `holes::several` |
| blind hole | supported | `hole 4 0,0 depth=5` | `holes::blind` |
| counterbore | missing | `hole 3.2 0,0 cbore=6,3` | `holes::counterbore` |
| countersink | missing | `hole 3.2 0,0 csink=6.4,90` | `holes::countersink` |
| tapped hole | missing | `hole 3.3 0,0 thread=M4` | `holes::threaded` |

## Fillets and chamfers

| operation | status | syntax | tests |
|---|---|---|---|
| fillet one edge | supported | `fillet 1 base.end&>Y` | `blends::fillet_edge` |
| fillet a chain with corners | supported | `fillet 1 base.end&base.side` | `blends::fillet_open_chain`, `blends::fillet_closed_chain_with_corners` |
| fillet a smooth chain | supported | rounded or circular edges | `blends::fillet_smooth_chain`, `blends::fillet_cylinder_rim` |
| fillet parallel edges | supported | `fillet 3 base.side&base.side` | `blends::fillet_vertical_edges` |
| fillet where three edges meet | missing | `fillet 2 all` | `blends::fillet_every_edge_of_a_box` |
| variable radius fillet | missing | `fillet 1 edges to=2` | `blends::fillet_variable_radius` |
| chamfer | supported | `chamfer 1 base.end&base.side` | `blends::chamfer_closed_chain`, `blends::chamfer_hole_rim` |
| chamfer with two distances | missing | `chamfer 1 edges d2=2` | `blends::chamfer_two_distances` |

## Bodies

| operation | status | syntax | tests |
|---|---|---|---|
| union | supported | `extrude` onto existing material | `bodies::union` |
| subtract | supported | `cut` | `bodies::subtract` |
| intersect | missing | `extrude 10 mode=intersect` | `bodies::intersect` |
| mirror | missing | `mirror YZ` | `bodies::mirror` |
| linear pattern | missing | `repeat h count=4 step=10,0` | `bodies::linear_pattern` |
| circular pattern | missing | `repeat h count=6 angle=360` | `bodies::circular_pattern` |
| move | missing | `move 10,0,0` | `bodies::translate` |
| rotate | missing | `rotate 90 axis=z` | `bodies::rotate` |
| scale | missing | `scale 2` | `bodies::scale` |
| split | missing | `split XY offset=5 keep=below` | `bodies::split` |
| separate bodies | missing | `body second` | `bodies::separate_bodies` |

## Output

| operation | status | syntax | tests |
|---|---|---|---|
| STEP export | supported | `linecad export part.lcad part.step` | `output::step`, `output::step_opens_in_opencascade` |
| STL export | supported | `linecad export part.lcad part.stl` | `output::stl` |
| PNG views | supported | `linecad render part.lcad part.png` | `output::png` |
| STEP import | missing | `import part.step` | `output::step_import` |

## Parameters and inspection

| operation | status | syntax | tests |
|---|---|---|---|
| variables and expressions | supported | `let w=40 h=w*3/4` | `inspect::variables` |
| face and edge selectors | supported | `label.group`, `>Z`, `+X`, `all`, `a,b`, `a&b`, `x\|y` | `inspect::groups_and_edges`, `inspect::directional_faces` |
| groups survive later edits | supported | | `inspect::groups_survive_later_cuts` |
| errors that say what to do | supported | | `inspect::errors` |
