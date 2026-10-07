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
| regular polygon | supported | `ngon 20 6` | `sketch::regular_polygon` |
| slot | supported | `slot 30 10` | `sketch::slot` |
| ellipse | supported | `ellipse 20 10` | `sketch::ellipse` |
| lines and arcs | supported | `pen`, `line`, `arc via=`, `close` | `sketch::lines_and_arcs` |
| spline | supported | `spline x,y ... closed` | `sketch::spline` |
| text | supported | `text LINECAD size=10` | `sketch::text` |
| polygon with rounded corners | supported | `poly ... r=4` | `sketch::rounded_polygon` |
| polygon with chamfered corners | supported | `poly ... c=2` | `sketch::chamfered_polygon` |
| offset outline | supported | `offset 2` | `sketch::offset_outline` |
| arc by centre | supported | `arc x,y center=x,y` | `sketch::arc_by_centre` |
| parallel, perpendicular and equal | supported | `parallel a b c d` | `sketch::rectangle_from_constraints` |
| midpoint, point on a line, coincident | supported | `midpoint m a b`, `online p a b`, `coincident a b` | `sketch::square_with_midpoint_and_online` |
| constraints and dimensions | supported | `point`, `dist`, `horizontal` | `sketch::constraints` |
| trim and extend | supported | `close trim` | `sketch::trim` |
| construction geometry | supported | `circle 20 construct` | `sketch::construction_geometry` |
| mirror inside a sketch | supported | `reflect y` | `sketch::sketch_mirror` |
| pattern inside a sketch | supported | `array count=4 angle=360` | `sketch::sketch_pattern` |
| DXF import | supported | `dxf file.dxf` | `sketch::dxf_import` |
| SVG import | supported | `svg file.svg` | `sketch::svg_import` |


## Workplanes

| operation | status | syntax | tests |
|---|---|---|---|
| XY, XZ, YZ | supported | `plane XZ` | `workplane::xy`, `workplane::xz`, `workplane::yz` |
| offset plane | supported | `plane XY offset=10` | `workplane::offset` |
| plane on a face | supported | `plane base.end` | `workplane::on_a_face`, `workplane::side_face_keeps_world_coordinates` |
| rotated plane | supported | `plane XY rx=90` | `workplane::rotated` |
| plane through three points | supported | `plane x,y,z x,y,z x,y,z` | `workplane::through_three_points` |
| plane at an angle to an edge | supported | `plane edge=a&b angle=30` | `workplane::at_an_angle_to_an_edge` |
| plane normal to a path | supported | `plane path at=0.5` | `workplane::normal_to_a_path` |
| datum axis | supported | `axis name x,y,z x,y,z` | `workplane::datum_axis` |


## Features

| operation | status | syntax | tests |
|---|---|---|---|
| extrude | supported | `extrude 5`, `extrude -5` | `features::extrude`, `features::extrude_reversed` |
| extrude with draft | supported | `extrude 10 draft=5` | `features::extrude_draft` |
| symmetric extrude | supported | `extrude 10 both` | `features::extrude_symmetric` |
| extrude up to a face | supported | `extrude upto=base.end` | `features::extrude_up_to_face` |
| cut | supported | `cut 4`, `cut thru` | `features::cut_blind`, `features::cut_through` |
| cut with draft | supported | `cut 4 draft=3` | `features::cut_draft` |
| revolve | supported | `revolve 360 axis=y`, `revolve 90` | `features::revolve_full`, `features::revolve_partial` |
| sweep along a path | supported | `path x,y,z ... r=8` then `sweep` | `features::sweep_bent_path`, `features::sweep_straight_path` |
| sweep along a helix | supported | `helix r=10 pitch=5 turns=3` then `sweep` | `features::sweep_helix` |
| loft | supported | `section` per profile, then `loft` | `features::loft` |
| shell | supported | `shell 2 open=base.end` | `features::shell` |
| face draft | supported | `draft 5 base.side neutral=base.start` | `features::face_draft` |
| push or pull a face | supported | `push base.end 2` | `features::push_face` |
| revolve cut | supported | `revolve 360 mode=cut` | `features::revolve_cut` |
| sweep cut | supported | `sweep mode=cut` | `features::sweep_cut` |
| loft cut | supported | `loft mode=cut` | `features::loft_cut` |
| thin extrude | supported | `extrude 10 thin=2` | `features::thin_extrude` |
| extrude to an offset from a face | supported | `extrude upto=face offset=-2` | `features::extrude_up_to_offset_face` |
| extrude to the next face | supported | `extrude next` | `features::extrude_to_next_face` |
| rib | supported | open `pen` path, then `rib 2` | `features::rib` |
| smooth loft | supported | `loft smooth` | `features::smooth_loft` |
| loft between different profiles | supported | `section`s with different edge counts | `features::loft_mixed_profiles` |
| sweep along a smooth path | supported | `path ... smooth` | `features::sweep_smooth_path` |
| sweep with twist | supported | `sweep twist=90` | `features::sweep_twist` |
| sweep with scale | supported | `sweep scale=0.5` | `features::sweep_scale` |
| modelled thread | missing | `thread M6 on=rod.side` | `features::modelled_thread` |
| shell with several openings | supported | `shell 2 open=a.end,a.start` | `features::shell_two_openings` |
| shell of any solid | supported | `shell 1` on a revolve | `features::shell_revolved` |
| draft curved faces | supported | `draft 5 base.side` on a cylinder | `features::draft_curved` |
| push in with curved sides | supported | `push base.end -2` on a cylinder | `features::push_curved` |
| wrap a sketch onto a face | supported | `wrap base.side depth=0.5 [raise]` | `features::wrap_text`, `features::wrap_rectangle` |
| thicken a face | supported | `thicken base.side 1` | `features::thicken_face` |
| union with coplanar faces | supported | same-size box on a face | `features::stack_same_size` |
| cut with coplanar faces | supported | notch flush with the sides | `features::notch_flush_with_sides` |
| union flush with some sides | missing | step on top, flush with three sides | `features::step_flush_with_sides` |


## Holes

| operation | status | syntax | tests |
|---|---|---|---|
| through hole | supported | `hole 4 x,y ...` | `holes::through`, `holes::several` |
| blind hole | supported | `hole 4 0,0 depth=5` | `holes::blind` |
| counterbore | supported | `hole 3.2 0,0 cbore=6,3` | `holes::counterbore` |
| countersink | supported | `hole 3.2 0,0 csink=6.4,90` | `holes::countersink` |
| tapped hole | supported | `hole 3.3 0,0 thread=M4` | `holes::threaded` |
| angled hole | supported | `plane >Z rx=30` then `hole` | `holes::angled` |
| hole on a curved face | supported | `hole 4 0,10 on=rod.side` | `holes::on_a_curved_face` |


## Fillets and chamfers

| operation | status | syntax | tests |
|---|---|---|---|
| fillet one edge | supported | `fillet 1 base.end&>Y` | `blends::fillet_edge` |
| fillet a chain with corners | supported | `fillet 1 base.end&base.side` | `blends::fillet_open_chain`, `blends::fillet_closed_chain_with_corners` |
| fillet a smooth chain | supported | rounded or circular edges | `blends::fillet_smooth_chain`, `blends::fillet_cylinder_rim` |
| fillet parallel edges | supported | `fillet 3 base.side&base.side` | `blends::fillet_vertical_edges` |
| fillet every edge of a flat-faced convex solid | supported | `fillet 2 all` | `blends::fillet_every_edge_of_a_box`, `blends::fillet_every_edge_of_a_prism` |
| fillet some of the edges where three meet | supported | `fillet 2 base.end&base.side\|base.side&base.side` | `blends::fillet_some_edges_at_a_corner`, `blends::fillet_three_edges_at_one_corner` |
| variable radius fillet | supported | `fillet 1 edges to=2` | `blends::fillet_variable_radius` |
| chamfer | supported | `chamfer 1 base.end&base.side` | `blends::chamfer_closed_chain`, `blends::chamfer_hole_rim` |
| chamfer with two distances | supported | `chamfer 1 edges d2=2` | `blends::chamfer_two_distances` |
| chamfer where three edges meet | supported | `chamfer 2 all` | `blends::chamfer_every_edge_of_a_box` |
| round corners on curved faces | supported | `fillet 1 all` on a rounded rect | `blends::fillet_every_edge_of_a_rounded_prism` |
| round inside corners where three edges meet | missing | `fillet 1 all` on an L shape | `blends::fillet_every_edge_of_an_l_shape` |
| variable radius along a chain | supported | `fillet 1 chain to=2` | `blends::fillet_variable_chain` |
| full round | supported | `fillet full base.end` | `blends::full_round` |


## Bodies

| operation | status | syntax | tests |
|---|---|---|---|
| union | supported | `extrude` onto existing material | `bodies::union` |
| subtract | supported | `cut` | `bodies::subtract` |
| intersect | supported | `extrude 10 mode=intersect` | `bodies::intersect` |
| mirror | supported | `mirror YZ` | `bodies::mirror` |
| linear pattern | supported | `repeat h count=4 step=10,0` | `bodies::linear_pattern` |
| circular pattern | supported | `repeat h count=6 angle=360` | `bodies::circular_pattern` |
| move | supported | `move 10,0,0` | `bodies::translate` |
| rotate | supported | `rotate 90 axis=z` | `bodies::rotate` |
| scale | supported | `scale 2` | `bodies::scale` |
| split | supported | `split XY offset=5 keep=below` | `bodies::split` |
| separate bodies | supported | `body second` | `bodies::separate_bodies` |
| booleans between bodies | supported | `combine main b mode=cut` | `bodies::combine_bodies` |
| mirror one feature | supported | `mirror YZ of=boss` | `bodies::mirror_feature` |
| pattern along a path | supported | `repeat h along=path count=4` | `bodies::pattern_along_path` |
| move or rotate a copy | supported | `move 20,0,0 copy` | `bodies::transform_copy` |
| scale unevenly | supported | `scale 2,1,1` | `bodies::scale_unevenly` |
| place one body on another | supported | `place lid on=>Z` | `bodies::assembly_mate` |
| interference check | supported | `measure overlap main b` | `bodies::interference` |

## Assemblies

Assemblies are `.lasm` files. They bring in the bodies of `.lcad` part files and join them.

| operation | status | syntax | tests |
|---|---|---|---|
| bring in a part file | supported | `part box box.lcad body=lid` | `assembly::parts_from_files` |
| set a part's variables | supported | `part plate plate.lcad w=size` | `assembly::part_variables` |
| geometry stays in part files | supported | `rect` in a `.lasm` fails | `assembly::geometry_stays_in_parts`, `assembly::part_errors_name_the_file` |
| concentric mate | supported | `concentric pin:pin.side plate:hole.side near=x,y,z` | `assembly::concentric_pin_in_a_hole`, `assembly::concentric_picks_the_hole_near_a_point` |
| flush mate | supported | `flush pin:pin.start plate:base.end offset=1` | `assembly::concentric_pin_in_a_hole` |
| parallel mate | supported | `parallel b:rod.end a:rod.end` | `assembly::parallel_and_angle_turn_parts`, `assembly::a_parallel_mate_stops_a_joint` |
| angle mate | supported | `angle lid:plate.end base:plate.end 30` | `assembly::parallel_and_angle_turn_parts` |
| distance mate | supported | `distance b:rod.side a:rod.side 50` | `assembly::distance_between_axes` |
| tangent mate | supported | `tangent rod:rod.side plate:plate.end` | `assembly::tangent_lays_a_rod_on_a_plate` |
| mates hold parts together | supported | a screw mated to lid and box stops the hinge | `assembly::a_screw_locks_the_hinge` |
| sub-assemblies | supported | `part kit boxed.lasm` keeps its joints and mates | `assembly::a_sub_assembly_keeps_its_mates` |
| check holes line up | supported | `aligned lid:screws.side box:pilot.side tol=0.05` | `assembly::holes_line_up` |
| turning joint | supported | `joint open box.lid box.main turn about=hinge at=-90` | `assembly::hinge_opens_the_lid` |
| sliding joint | supported | `joint pull chest.drawer chest.main slide along=0,-1,0 max=30` | `assembly::slide_moves_a_drawer` |
| cylindrical joint | supported | a `turn` and a `slide` on one axis | `assembly::a_rod_turns_and_slides_on_one_axis` |
| closed loops of joints | supported | a four-bar linkage follows its crank | `assembly::a_four_bar_linkage_follows_its_crank` |
| gears and racks | supported | `couple jb ja ratio=-0.5` | `assembly::coupled_joints_move_together` |
| posing a joint | supported | `pose open -90` | `assembly::pose_moves_children` |
| check every pair of parts | supported | `interference none` | `assembly::clear_assembly`, `assembly::strict_interference_fails` |
| pattern parts | supported | `pattern s1 holes=case.lid:screws.side`, `count=` `step=`/`angle=` | `assembly::pattern_copies_a_part_and_its_mates` |
| exploded view | supported | `explode lid 0,0,30`, `render --explode` | `assembly::exploded_views_move_parts_apart` |
| assembly drawing with parts list | supported | `linecad export top.lasm top.svg` | `output::assembly_drawing_lists_parts` |
| bill of materials | supported | `linecad bom top.lasm [--csv]` | `assembly::bill_of_materials_counts_parts` |
| sweep a joint for clashes | supported | `interference joint=open steps=8` | `assembly::sweep_finds_a_clash` |

## Output

| operation | status | syntax | tests |
|---|---|---|---|
| STEP export | supported | `linecad export part.lcad part.step` | `output::step`, `output::step_opens_in_opencascade` |
| STL export | supported | `linecad export part.lcad part.stl` | `output::stl` |
| PNG views | supported | `linecad render part.lcad part.png` | `output::png` |
| STEP import | supported | `import part.step` | `output::step_import` |
| OBJ export | supported | `linecad export part.lcad part.obj` | `output::obj` |
| 3MF export | supported | `linecad export part.lcad part.3mf` | `output::three_mf` |
| SVG drawing | supported | `linecad export part.lcad part.svg` | `output::drawing` |
| drawing dimensions and hole callouts | supported | overall sizes, `4× ⌀3.2` | `output::drawing_dimensions_and_section` |
| section view | supported | `linecad export part.lcad part.svg --section y=0` | `output::drawing_dimensions_and_section` |
| STL import | supported | `import part.stl` | `output::stl_import` |
| STEP colours | supported | `color red` | `output::step_colours` |
| STEP assembly structure | supported | `linecad export top.lasm top.step` | `output::step_assembly`, `output::step_assembly_opens_in_opencascade` |


## Parameters and inspection

| operation | status | syntax | tests |
|---|---|---|---|
| variables and expressions | supported | `let w=40 h=w*3/4` | `inspect::variables` |
| math functions | supported | `let a=atan2(3,4) r=sqrt(x)` | `inspect::functions` |
| face and edge selectors | supported | `label.group`, `>Z`, `+X`, `all`, `a,b`, `a&b`, `x\|y` | `inspect::groups_and_edges`, `inspect::directional_faces` |
| groups survive later edits | supported | | `inspect::groups_survive_later_cuts` |
| errors that say what to do | supported | | `inspect::errors` |
| measure distance | supported | `measure faces faces` | `inspect::measure_distance` |
| mass properties | supported | `measure mass` | `inspect::mass_properties` |
| materials and mass | supported | `material steel`, `material pla density=1.24` | `inspect::material_mass`, `assembly::materials_follow_parts` |
| wall thickness | supported | `measure thickness` | `inspect::wall_thickness` |
| draft analysis | supported | `measure draft pull=z` | `inspect::draft_analysis` |
| variables from outside the file | supported | `linecad check part.lcad --set w=20` | `inspect::outside_variables` |
| include another file | supported | `include part.lcad d=10` | `inspect::include_file` |
| conditional lines | supported | `if w>30 chamfer 1 edges` | `inspect::conditional` |
