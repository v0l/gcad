# Operations and their tests

Every operation a parametric CAD program is expected to have, whether gcad has it,
the line that does it, and the tests that prove it. Each test builds a part and checks
it against a volume or bounding box worked out by hand.

A `missing` row's tests are already written with the syntax the operation should take
and are marked `#[ignore = "missing: ..."]`. Building the operation means making those
tests pass and flipping the row to `supported`. `tests/ops_doc.rs` fails if this table
and the tests disagree: a test not listed here, a listed test that does not exist, a
`supported` row whose test is ignored as missing, or a `missing` row whose test runs.

Run everything with `cargo test --release`. Tests that need a GPU (`flow`, `simulation::wind_*`)
are ignored unless you add `--features gpu-tests`. The OpenCascade check needs a Python with
OCP: `GCAD_OCP_PYTHON=~/git/cadkit/.venv/bin/python cargo test --release -- --ignored step_opens`.

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
| involute spur gear | supported | `gear 20 2 at=x,y` | `sketch::involute_gear` |
| internal gear | supported | `circle 80` then `gear 54 1 internal` | `sketch::internal_gear` |
| rack | supported | `rack 10 2 back=3` | `sketch::rack` |
| ellipse | supported | `ellipse 20 10` | `sketch::ellipse` |
| lines and arcs | supported | `pen`, `line`, `arc via=`, `close` | `sketch::lines_and_arcs` |
| spline | supported | `spline x,y ... closed` | `sketch::spline` |
| text | supported | `text GCAD size=10` | `sketch::text` |
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
| cut from a sketch inside the solid | supported | sketch plane below the top, or beside a taller block | `features::cut_from_inside_a_cylinder`, `features::cut_beside_a_taller_block` |
| revolve | supported | `revolve 360 axis=y`, `revolve 90` | `features::revolve_full`, `features::revolve_partial` |
| sweep along a path | supported | `path x,y,z ... r=8` then `sweep` | `features::sweep_bent_path`, `features::sweep_straight_path` |
| sweep along a helix | supported | `helix r=10 pitch=5 turns=3` then `sweep` | `features::sweep_helix` |
| loft | supported | `section` per profile, then `loft` | `features::loft`, `features::loft_many_matching_edges` |
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
| smooth loft with many edges | supported | `loft smooth` between sections of over 240 edges | `features::loft_many_matching_edges` |
| sweep along a smooth path | supported | `path ... smooth` | `features::sweep_smooth_path` |
| sweep with twist | supported | `sweep twist=90` | `features::sweep_twist` |
| sweep with scale | supported | `sweep scale=0.5` | `features::sweep_scale` |
| twisted or tapered extrude | supported | `extrude 10 twist=90`, `extrude 10 scale=0.5` | `features::extrude_twisted`, `features::extrude_scaled`, `features::twisted_extrude_with_a_hole` |
| helical gear | supported | `gear 20 2 helix=20` then `extrude 10` | `features::helical_gear` |
| bevel gear | supported | `gear 20 2 cone=45` then `extrude 5` | `features::bevel_gear` |
| modelled thread | supported | `thread M6 on=rod.side`, `thread M6 on=h.side` in a hole, `left`, `pitch=` | `features::modelled_thread`, `features::tapped_thread`, `features::thread_up_to_a_bolt_head`, `features::thread_on_an_undersized_rod_with_a_fine_pitch`, `features::thread_turns_right_handed`, `features::thread_needs_a_round_face_that_fits` |
| thread into a chamfer or countersink | supported | `chamfer 1 rod.end&rod.side` then `thread M6 on=rod.side` | `features::thread_runs_out_into_chamfers`, `features::left_hand_thread_runs_out_into_a_chamfer`, `features::tapped_thread_under_a_countersink` |
| shell with several openings | supported | `shell 2 open=a.end,a.start` | `features::shell_two_openings` |
| shell of any solid | supported | `shell 1` on a revolve | `features::shell_revolved` |
| shell after rounding | supported | `fillet 3 all` then `shell 2 open=base.end` | `features::shell_follows_rounded_corners`, `features::shell_as_thick_as_the_rounding`, `features::shell_follows_a_rounded_floor` |
| draft curved faces | supported | `draft 5 base.side` on a cylinder | `features::draft_curved` |
| push in with curved sides | supported | `push base.end -2` on a cylinder | `features::push_curved` |
| wrap a sketch onto a face | supported | `wrap base.side depth=0.5 [raise]` | `features::wrap_text`, `features::wrap_rectangle` |
| thicken a face | supported | `thicken base.side 1` | `features::thicken_face` |
| union with coplanar faces | supported | same-size box on a face | `features::stack_same_size` |
| cut with coplanar faces | supported | notch flush with the sides | `features::notch_flush_with_sides` |
| cut with a wall at the start of a corner fillet | supported | `cut` pocket whose wall sits on a rounded rectangle's fillet tangent | `features::cut_where_the_tool_wall_meets_a_corner_fillet_start` |
| cut with a circle on a round wall | supported | `circle D` on a ring of diameter D, or on its bore | `features::cut_on_an_outer_wall`, `features::cut_on_a_bore` |
| coplanar faces fused after a boolean | supported | a boss flush with a wall leaves one face | `inspect::flush_unions_leave_one_face_per_plane` |
| union flush with some sides | supported | step on top, flush with three sides | `features::step_flush_with_sides` |


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
| exact circles on hole rims | supported | any `hole` or round `cut` | `holes::hole_rims_are_exact_circles` |
| clearance hole for a screw | supported | `hole M3 x,y`, `fit=close\|normal\|loose` | `holes::clearance_for_a_screw` |
| tap drill and heat-set insert holes | supported | `hole M4 x,y fit=tap`, `hole M3 x,y fit=insert` | `holes::tapped_and_insert_holes_for_a_screw` |
| counterbore or countersink for a screw | supported | `cbore=M3`, `csink=M4` | `holes::counterbore_and_countersink_for_a_screw` |
| hole fits | supported | `hole 6 x,y fit=H7` | `holes::hole_fit_limits`, `output::drawing_notes_fits_and_taps` |
| o-ring sizes | supported | AS568 -004 to -284 and ISO 3601-1 1.8 to 7.0 mm series from `crates/gcad-calc/src/data` | `calc::as568_dash_numbers_match_the_handbook`, `calc::as568_inside_diameters_agree_with_the_inch_table`, `calc::iso_sizes_are_found_by_id_and_cross_section`, `calc::every_ring_belongs_to_a_cross_section_group`, `calc::gland_tables_cover_every_cross_section` |
| o-ring calculator | supported | `gcad calc oring piston 50 --pressure 10 --formulas` | `calc::face_seal_squeeze_matches_the_handbook_range`, `calc::piston_gland_uses_the_reciprocating_depth`, `calc::rod_groove_sits_outside_the_rod`, `calc::stretch_over_three_percent_is_rejected`, `calc::fill_over_eighty_five_percent_is_rejected`, `calc::fill_matches_the_area_ratio`, `calc::back_up_ring_is_flagged_above_the_handbook_pressure`, `calc::silicone_halves_the_allowed_clearance`, `calc::a_wide_gap_asks_for_a_back_up_ring`, `calc::lead_in_chamfer_is_twenty_degrees`, `calc::report_has_a_why_line_formulas_and_sources`, `calc::bad_inputs_are_refused`, `calc::calc_oring_prints_a_result_and_a_why_line`, `calc::calc_oring_refuses_a_bad_application` |
| thread, fit and material calculators | supported | `gcad calc thread M6 --material alu --depth 14`, `gcad calc fit H7/g6 25`, `gcad calc material petg` | `calc_thread::h7_g6_at_25`, `calc_thread::h7_p6_is_an_interference_fit`, `calc_thread::h7_h6_at_10_sits_on_the_band_edge`, `calc_thread::h11_c11_and_f7_h6`, `calc_thread::transition_fit_and_hole_letters_with_delta`, `calc_thread::js_is_symmetric`, `calc_thread::bad_classes_are_refused`, `calc_thread::report_carries_notes_and_sources`, `calc_thread::cli_prints_the_fit`, `calc_thread::m6_dimensions`, `calc_thread::fine_pitch_is_found_by_pitch`, `calc_thread::clearance_and_heads`, `calc_thread::unified_and_pipe_threads`, `calc_thread::torque_for_m6_class_8_8`, `calc_thread::engagement_by_material`, `calc_thread::blind_hole_depth_check`, `calc_thread::cli_thread_report`, `calc_thread::a_hole_shorter_than_one_diameter_is_flagged`, `calc_thread::a_deep_enough_hole_is_not`, `calc_thread::iso_273_clearance_holes_match_two_sources`, `calc_thread::socket_head_counterbores_match_freecad_iso_4762`, `calc_thread::countersunk_and_button_heads`, `calc_thread::fine_pitch_tap_drills_are_diameter_minus_pitch`, `calc_thread::coarse_tap_drills_match_iso_2306`, `calc_thread::pipe_thread_drills_and_pitches`, `calc_thread::unified_drills_are_within_a_sixth_of_a_millimetre_of_freecad`, `calc_thread::proof_stress_gives_iso_898_proof_loads`, `calc_thread::iso_286_limits_at_several_sizes`, `calc_thread::hole_letters_with_delta_match_roymech`, `calc_thread::materials_match_their_sources`, `calc_thread::every_material_has_properties_and_engagement`, `calc_thread::cli_material` |
| o-ring groove | supported | `oring_groove face 40 pressure=10`, `oring_groove piston 50 offset=10` | `calc::face_groove_volume_matches_the_maths`, `calc::piston_groove_volume_matches_the_maths`, `calc::rod_groove_cuts_the_bore_wall`, `calc::a_groove_the_ring_cannot_fill_is_refused` |
| o-ring standard part | supported | `part seal oring:214`, `oring:40x2.65` | `calc::oring_standard_part_is_a_torus_of_the_listed_size` |


## Enclosures

| operation | status | syntax | tests |
|---|---|---|---|
| lip on a rim | supported | `lip >Z 3 w=1` | `features::lip_on_a_rim` |
| groove on a rim | supported | `groove <Z 3 w=1 gap=0.2` | `features::groove_on_a_rim` |
| screw boss | supported | `boss 7 20 x,y ... hole=M3 fit=insert ribs=4` | `features::screw_boss`, `features::boss_for_an_insert` |
| vents through one wall | supported | `vent 20 3 x,y count=3 step=0,-5` | `features::vents_through_one_wall` |
| snap hook | supported | `snap 8 1.5 6 x,y hook=1.5 dir=90` | `features::snap_hook` |
| case and lid with screws | supported | `examples/assemblies/case.gasm` | `assembly::case_and_lid_with_screws` |

## Sheet metal

| operation | status | syntax | tests |
|---|---|---|---|
| sheet thickness, bend radius and K-factor | supported | `sheet 1.5 r=1.5 k=0.44` | `features::sheet_flange` |
| base tab | supported | `rect 40 30` then `base: tab` | `features::sheet_flange` |
| edge flange | supported | `flange base.end&>X 10`, `angle=` | `features::sheet_flange`, `features::sheet_flange_at_an_angle` |
| shorter flange and hem | supported | `flange e 10 inset=5`, `inset=2,8`, `angle=180` | `features::sheet_flange_inset_and_hem` |
| flange on a flange | supported | `flange >X&walls.end 5` | `features::sheet_flange_on_a_flange` |
| flat pattern | supported | `unfold`, holes and through cuts carried over | `features::sheet_unfolds` |
| bend a flat blank | supported | `bend a,b c,d angle=-90`, labels carried over, unfolds to the blank | `features::sheet_bends_a_flat_blank` |
| blank of any outline | supported | `slot` or arcs and holes, then `tab`, `bend`; DXF from the blank | `features::sheet_blank_with_any_outline` |
| pressed darts | supported | `dart fold -7 7 l=20 w=8` | `features::sheet_darts_stiffen_a_bend` |
| flat pattern DXF | supported | `gcad export part.gcad flat.dxf` | `output::flat_pattern_dxf` |

## Weldments

| operation | status | syntax | tests |
|---|---|---|---|
| structural members | supported | `member tube 40x40x3 x,y,z x,y,z ...`, `bar`, `pipe`, `rod`, `angle`, `channel` | `features::structural_members`, `features::member_materials`, `features::member_pattern_preserves_library_material` |
| member patterns | supported | `member bar 44x175 ... material=c24 count=19 step=400,0,-10`, `end_step=`, `when=`, `count=8 angle=360 axis=hub` | `features::linear_member_pattern`, `features::circular_member_pattern`, `features::member_row_tapers_and_skips` |
| mitred frame and cut list | supported | `member tube 40x40x3 ... closed`, `gcad bom` | `features::mitred_frame` |

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
| fillet every edge of a plate with holes | supported | `fillet 1 all` after `hole` | `blends::fillet_every_edge_of_a_plate_with_holes` |
| round inside corners where three edges meet | supported | `fillet 1 all` on an L shape, a pocket or a boss | `blends::fillet_every_edge_of_an_l_shape`, `blends::fillet_every_edge_of_a_pocket`, `blends::fillet_every_edge_of_a_boss`, `blends::fillet_inside_edge_ending_at_a_sharp_corner`, `blends::fillet_pocket_floor_with_sharp_walls`, `blends::chamfer_every_edge_of_an_l_shape` |
| variable radius along a chain | supported | `fillet 1 chain to=2` | `blends::fillet_variable_chain` |
| full round | supported | `fillet full base.end` | `blends::full_round` |


## Bodies

| operation | status | syntax | tests |
|---|---|---|---|
| union | supported | `extrude` onto existing material | `bodies::union`, `features::boss_glued_onto_a_face`, `features::boss_on_a_twisted_face`, `features::boss_over_an_edge_still_joins` |
| subtract | supported | `cut` | `bodies::subtract`, `bodies::thru_cut_across_thin_sheet_rows` |
| intersect | supported | `extrude 10 mode=intersect` | `bodies::intersect` |
| mirror | supported | `mirror YZ` | `bodies::mirror` |
| linear pattern | supported | `repeat h count=4 step=10,0` | `bodies::linear_pattern` |
| circular pattern | supported | `repeat h count=6 angle=360` | `bodies::circular_pattern` |
| move | supported | `move 10,0,0` | `bodies::translate` |
| rotate | supported | `rotate 90 axis=z` | `bodies::rotate`, `assembly::a_transform_in_a_later_body_keeps_earlier_bodies_groups` |
| scale | supported | `scale 2` | `bodies::scale` |
| split | supported | `split XY offset=5 keep=below` | `bodies::split`, `bodies::split_either_side` |
| separate bodies | supported | `body second` | `bodies::separate_bodies` |
| booleans between bodies | supported | `combine main b mode=cut` | `bodies::combine_bodies` |
| point separate bodies at an assembly | supported | `gcad check` on a `.gcad` left with two bodies | `bodies::check_sends_separate_parts_to_an_assembly` |
| mirror one feature | supported | `mirror YZ of=boss` | `bodies::mirror_feature` |
| pattern along a path | supported | `repeat h along=path count=4` | `bodies::pattern_along_path` |
| move or rotate a copy | supported | `move 20,0,0 copy` | `bodies::transform_copy` |
| scale unevenly | supported | `scale 2,1,1` | `bodies::scale_unevenly` |
| place one body on another | supported | `place lid on=>Z` | `bodies::assembly_mate` |
| interference check | supported | `measure overlap main b` | `bodies::interference` |

## Meshes

A mesh body is triangles, for organic shapes and game assets. It is blocked out from
blended primitives, turned into a mesh by `remesh`, and sculpted with `select` and the ops
after it. Each test checks a volume or bounding box worked out by hand.

| operation | status | syntax | tests |
|---|---|---|---|
| ellipsoid blob | supported | `head: blob 0,0,30 r=5,4,6 rz=20` | `mesh::blob_sphere`, `mesh::blob_ellipsoid_turned` |
| rounded box block | supported | `plank: block 0,0,5 size=20,4,2 round=0.5 rz=30` | `mesh::block_is_a_rounded_box`, `mesh::block_turns_and_blends` |
| limb through joints | supported | `arm: limb 0,0,0 0,0,20 r=3,2` | `mesh::limb_capsule` |
| smooth blend between primitives | supported | `blob ... blend=2` | `mesh::blend_fills_the_join` |
| carve a blob out | supported | `blob ... mode=cut` | `mesh::blob_cut` |
| mirror a blockout part | supported | `mirror x of=ear_l` | `mesh::mirror_blockout_names_the_other_side` |
| remesh to triangles | supported | `remesh 0.3` | `mesh::blob_sphere` |
| select a named part | supported | `select head` | `mesh::select_part_moves_only_that_part`, `mesh::select_unknown_part_lists_the_parts` |
| select a region with falloff | supported | `select sphere=x,y,z r=4 falloff=6` | `mesh::select_falloff_keeps_far_vertices_still` |
| select both sides | supported | `select ... mirror=x` | `mesh::select_mirror_edits_both_sides` |
| move, scale, rotate a selection | supported | `move 0,0,3`, `scale 1.2`, `rotate 30 axis=z` | `mesh::select_part_moves_only_that_part`, `mesh::scale_selection_about_its_centre`, `mesh::rotate_selection` |
| inflate along normals | supported | `inflate 1` | `mesh::inflate_grows_a_sphere` |
| smooth | supported | `smooth 10` | `mesh::smooth_rounds_a_box_edge` |
| refine a region | supported | `refine 0.1` | `mesh::refine_hits_the_edge_length_and_stays_closed`, `mesh::refine_only_inside_the_selection`, `mesh::refine_follows_the_curve` |
| refine rings at joints | supported | `refine joints edge=0.3 reach=1.8` | `rig::refine_joints_splits_triangles_near_the_joint_only`, `rig::refine_joints_needs_a_parented_bone` |
| crease a groove | supported | `crease x,y,z x,y,z depth=0.3 width=0.4` | `mesh::crease_cuts_a_groove_of_the_given_depth`, `mesh::crease_drawn_just_off_the_surface_still_cuts` |
| pinch toward a point | supported | `pinch 0.5 toward=x,y,z` | `mesh::pinch_draws_vertices_together_along_the_surface`, `mesh::pinch_with_mirror_pulls_each_side_to_its_own_point` |
| flatten onto a plane | supported | `flatten at=0,0,0 normal=z`, `flatten cut ...` | `mesh::flatten_onto_a_plane`, `mesh::flatten_cut_trues_a_sole_without_crushing_the_paw` |
| surface noise | supported | `noise 0.2 size=1 seed=3` | `mesh::noise_is_repeatable_and_bounded` |
| relax triangles | supported | `relax 10` | `mesh::relax_evens_triangles_without_shrinking` |
| decimate to a budget | supported | `decimate 5000`, `decimate 25%` | `mesh::decimate_to_a_budget_keeps_the_shape`, `mesh::decimate_by_share_leaves_unselected_triangles`, `mesh::decimate_keeps_part_labels`, `mesh::decimate_keeps_the_border_between_parts_fine` |
| colour a part | supported | `color #ffffff part=chest gloss=0.8` | `mesh::colour_one_part`, `mesh::colour_unknown_part_lists_the_parts`, `mesh::gloss_goes_to_the_mtl`, `mesh::soft_colour_fades_across_the_border`, `mesh::obj_export_writes_soft_colours_per_vertex` |
| a separate closed shell | supported | `eye_l: blob 1,-5,2 r=0.5 mode=shell` | `mesh::shell_is_its_own_closed_surface` |
| a game-ready fox | supported | `examples/mesh/fox.gcad` | `mesh::fox_example_fits_a_game_budget` |
| inspect a view to place detail | supported | `gcad inspect f.gcad out.png --part head --view right --pick 300,412` | `inspect_view::pick_returns_the_surface_point_and_normal`, `inspect_view::pixel_and_world_agree_in_every_view`, `inspect_view::part_frames_just_that_part`, `inspect_view::marks_say_when_the_surface_hides_them`, `inspect_view::unknown_part_lists_the_parts`, `inspect_view::solids_show_too` |
| import a mesh | supported | `import file.obj mesh` | `mesh::mesh_import_obj` |
| export a mesh with its parts | supported | `gcad export file.gcad out.obj` | `mesh::mesh_export_obj_keeps_parts` |
| mesh and solid bodies in one file | supported | `body figure` then `blob` | `mesh::mesh_and_solid_bodies_share_a_file`, `mesh::solid_ops_refuse_a_mesh_body` |
| a creature, blocked out and sculpted | supported | `examples/mesh/creature.gcad` | `mesh::creature_example` |
| bones in a hierarchy | supported | `bone knee parent=thigh tail=0,0,5` | `rig::bones_build_a_hierarchy`, `rig::bone_errors_name_the_bones` |
| mirror a bone chain | supported | `mirror x of=arm_l` | `rig::mirror_bone_makes_the_other_side` |
| list the skeleton | supported | `gcad inspect f.gcad out.png` | `rig::inspect_lists_the_skeleton`, `rig::inspect_report_lists_the_skeleton` |
| automatic skin weights | supported | `skin auto` | `rig::skin_auto_weights_sum_to_one_with_four_influences_at_most`, `rig::skin_auto_binds_each_end_to_its_own_bone`, `rig::skin_auto_does_not_bind_the_body_to_an_arm_beside_it`, `rig::skin_auto_does_not_bind_a_leg_to_a_hand_hanging_beside_it` |
| bind a part to a bone | supported | `skin part=upper bone=b_up` | `rig::skin_part_moves_only_with_its_bone`, `rig::skin_part_unknown_lists_the_parts` |
| paint skin weights | supported | `skin paint bone=b sphere=0,0,10 r=4 falloff=3`, `skin paint bone=b sphere=0,0,6 0,0,10 r=3 strength=-0.5 mirror=x` | `rig::skin_paint_moves_weight_to_the_bone`, `rig::skin_paint_takes_a_stroke_and_negative_strength_gives_weight_back` |
| smooth skin weights | supported | `skin smooth sphere=0,0,20 r=5 passes=40` | `rig::skin_smooth_softens_the_seam_between_two_parts` |
| mirror and normalise skin weights | supported | `skin mirror`, `skin normalize` | `rig::skin_mirror_copies_one_side_with_bones_swapped` |
| check skin quality | supported | `gcad skincheck creature.gcad` | `rig::skincheck_reports_the_creature` |
| twist bone segments | supported | `bone fore_l parent=upper_l tail=12,0,0 twist=3` | `rig::twist_segments_share_the_roll_and_mirror` |
| dual quaternion skinning | supported | `skin blend dq=1` | `rig::skin_blend_dq_keeps_a_half_turn_round` |
| pose a bone | supported | `pose bone=knee rotate=30 axis=x`, `pose reset` | `rig::an_unposed_rig_equals_the_rest_mesh`, `rig::a_bent_limb_stays_closed_and_keeps_its_volume`, `rig::pose_rotates_about_the_bone_head`, `rig::pose_names_the_bones_when_it_misses`, `rig::pose_without_skin_leaves_the_mesh_alone` |
| corrective shape (pose-driven morph target) | supported | `corrective elbow bone=lower_b axis=x from=60 to=120` then sculpt lines such as `select sphere=0,-2,12 r=3 falloff=5`, `move 0,2,0` | `rig::corrective_ramps_with_the_bone_angle_and_reaches_the_glb`, `rig::corrective_errors_name_the_problem` |
| animation clips | supported | `clip walk fps=24`, `key t=0.5 bone=hind_l rotate=25 axis=x`, `pose clip=walk t=0.25` | `rig::clip_keys_are_sampled_between_keyframes`, `rig::key_without_a_clip_is_refused` |
| clip translation keys | supported | `key t=0.3 bone=hips move=0,0,4` | `rig::move_keys_shift_a_bone_and_reach_the_glb` |
| glTF binary export with skin and clips | supported | `gcad export creature.gcad creature.glb` | `rig::glb_carries_skin_weights_and_animations`, `rig::glb_without_a_rig_has_no_skin`, `rig::glb_exports_from_a_file_and_obj_is_posed`, `rig::glb_colours_are_linear`, `rig::glb_carries_solid_bodies_in_their_colours`, `rig::a_solid_part_file_exports_to_glb` |
| a rigged creature with two clips | supported | `examples/mesh/rigged_creature.gcad` | `rig::rigged_creature_example_poses_and_exports` |
| build a mesh and rig without the CAD kernel | supported | `gcad2glb in.gcad out.glb`, `gcad_mesh::build(source)` | `gcad2glb::rigged_creature_matches_gcad_export_byte_for_byte`, `gcad2glb::creature_matches_gcad_export_byte_for_byte`, `gcad2glb::fox_matches_gcad_export_byte_for_byte`, `gcad2glb::same_spec_gives_identical_bytes`, `gcad2glb::bone_matrices_bend_the_skinned_positions` |
| solid operations are refused with a line number in the mesh library | supported | `gcad_mesh::build("rect 1 1")` | `gcad2glb::solid_operations_are_refused_with_the_line_number`, `gcad2glb::bad_arguments_name_their_line` |
| lower remesh detail for slow devices | supported | `gcad2glb --detail 3 in.gcad out.glb` | `gcad2glb::lower_detail_gives_fewer_triangles` |
| build solid and mesh files at runtime in a game | supported | `gcad2glb::Assets::from_path("prop.gcad", &Options::default().set("w", 90.0))` | `gcad2glb::the_library_matches_export_with_set_and_scale_on_a_solid_part` |
| one GLB from a file with solid and mesh bodies, without the app | supported | `gcad2glb bracket_mascot.gcad out.glb` (gcad2glb's default `brep` feature) | `gcad2glb::gcad2glb_writes_solids_and_meshes_like_export`, `gcad2glb::flat_solids_match_export_through_the_tool_and_the_library` |
| mesh-only gcad2glb build | supported | `cargo build -p gcad2glb --no-default-features` | `gcad2glb::light_gcad2glb_refuses_solids_with_the_line_and_matches_on_meshes` |
| define a brush | supported | `brush clay type=clay size=0.4 strength=0.1 falloff=smooth focal=0.3 edge=0.5 spacing=0.2 lazy=0.5 mirror=x` | `brush::dab_draw_raises_by_strength_with_a_linear_falloff`, `brush::brush_edge_keeps_a_flat_core`, `brush::brush_errors_name_the_choices` |
| one brush dab on the surface | supported | `dab clay at=x,y,z` | `brush::dab_draw_raises_by_strength_with_a_linear_falloff`, `brush::a_dab_off_the_surface_lands_on_it_and_says_how_far` |
| a stroke along a path | supported | `stroke clay from=a to=b`, `stroke clay x,y,z x,y,z ... pressure=0,1` | `brush::stroke_lays_a_ridge_along_its_path`, `brush::stroke_pressure_ramps_the_ridge`, `brush::stroke_points_and_lazy_smooth_a_shaky_path` |
| draw, clay, crease, pinch, flatten, smooth brushes | supported | `type=draw`, `clay`, `dam_standard`, `pinch`, `planar`, `smooth` | `brush::clay_builds_up_to_a_plane_above_the_surface`, `brush::crease_brush_cuts_a_groove_of_its_strength`, `brush::pinch_brush_draws_vertices_to_the_stroke`, `brush::flatten_brush_planes_the_top`, `brush::smooth_brush_calms_noise_only_under_it` |
| move and snake hook brushes | supported | `type=move`, `type=snake_hook detail=0.25` | `brush::move_brush_drags_a_region_by_the_stroke`, `brush::snake_hook_pulls_out_a_spike` |
| greyscale PNG alpha stamp | supported | `brush s type=stamp alpha=stamp.png(0.8)` | `brush::png_alpha_stamp_dents_like_a_procedural_one`, `brush::ramp_png_stamp_dent_profile`, `brush::missing_png_names_the_file` |
| brush size in pixels of the last inspect | supported | `brush b type=draw size=40px` | `brush::px_size_needs_an_inspect_view_then_uses_it`, `brush::px_size_is_pixels_times_units_per_pixel_from_the_sidecar`, `brush::px_size_warns_when_the_file_changed_after_the_inspect` |
| alpha stamps | supported | `type=stamp alpha=pores(0.8,2)`, `scales`, `wrinkles`, `stitch`, `hair`, `dots`, `noise` | `brush::alpha_stamp_dents_and_raises_within_its_strength` |
| mirrored and geodesic brushes | supported | `brush ... mirror=x geodesic=on` | `brush::mirrored_brush_raises_both_sides_alike`, `brush::geodesic_brush_stays_on_its_side_of_a_thin_shell` |
| mask vertices | supported | `mask sphere=x,y,z r=`, `mask part=`, `mask by=height axis=z from= to=`, `mask by=cavity r=1` | `brush::masked_vertices_stay_still`, `brush::mask_by_height_ramps_between_two_levels`, `brush::mask_by_cavity_finds_a_groove` |
| change the mask | supported | `mask invert`, `mask clear`, `mask blur 3`, `mask grow 0.5`, `mask shrink 0.5` | `brush::brushes_respect_the_mask_and_invert_and_clear_it`, `brush::mask_grow_widens_and_shrink_narrows` |
| hold parts still | supported | `hide ear_l`, `isolate head`, `show` | `brush::isolate_and_hide_keep_other_parts_still` |
| polygroups | supported | `group cap sphere=x,y,z r=2` then `select cap` | `brush::group_makes_a_polygroup_to_select` |
| selection falloff shape and strength | supported | `select sphere=... falloff=2 profile=linear strength=0.5` | `brush::select_profile_and_strength_shape_the_falloff` |
| soften the seam between parts | supported | `join arm body blend=1.5` | `brush::join_softens_the_seam_between_parts` |
| dynamic topology under brushes | supported | `dyntopo on detail=0.05` | `brush::dyntopo_refines_under_the_brush` |
| subdivide and project to a stored level | supported | `level base`, `subdivide 2`, `project base` | `brush::subdivide_quadruples_the_triangles`, `brush::project_returns_a_selection_to_a_stored_level` |
| sculpt layers | supported | `layer wrinkles`, then `layer wrinkles amount=0.5` | `brush::layer_amount_dials_its_strokes` |
| even remesh of a sculpt | supported | `remesh dynamesh 0.25` | `brush::dynamesh_remeshes_a_sculpt_evenly_and_keeps_its_shape` |
| finish at full detail, then decimate | supported | `finish decimate=20000`, `finish decimate=10%`, `finish max_error=0.02` | `brush::finish_decimates_to_a_budget_and_reports_the_error`, `brush::finish_to_an_error_bound_stays_within_it`, `brush::finish_keeps_fine_brush_detail_better_than_a_coarse_build` |
| keep more triangles where they matter | supported | `finish decimate=14000 focus=head,jaw weight=30` | `brush::finish_focus_spends_the_budget_on_the_named_parts` |
| keep creases, colour borders and UV charts while decimating | supported | `finish decimate=14000 keep=parts,creases,colours,uv crease=40` | `brush::finish_keep_creases_holds_the_edges_of_a_cube_sharper` |
| bake sculpt detail to a normal map | supported | `level full` ... `finish decimate=14000`, `bake normals size=1024` | `brush::bake_normals_writes_a_tangent_space_map_that_tilts_on_the_wrinkle`, `brush::a_file_without_bake_has_no_uvs_or_images`, `brush::skin_weights_and_uvs_survive_finish_in_the_same_primitive` |
| LODs in the GLB | supported | `lod 8000 3000 1000`, `lod 40% 10%` | `brush::lod_writes_three_meshes_with_the_asked_triangle_counts`, `brush::lod_shares_count_from_the_full_detail_mesh`, `brush::lod_and_bake_refuse_nonsense`, `gcad2glb::export_and_gcad2glb_write_the_same_bytes_for_a_bake_and_lod` |
| a sculpted, rigged hero | supported | `examples/mesh/hero.gcad` | `brush::hero_example_sculpts_at_full_detail_and_finishes_to_its_budget` |
| export budget | supported | `gcad export f.gcad f.glb --tris 20000`, `--max-error 0.02`, `gcad2glb --tris 20000` | `gcad2glb::a_triangle_budget_finishes_every_body_at_export`, `gcad2glb::an_error_bound_keeps_the_surface_within_it`, `gcad2glb::export_tris_flag_matches_gcad2glb` |
| fit a GLB to a game's units | supported | `gcad export f.gcad f.glb --height 1 --centre`, `--scale 0.01`, `gcad2glb --height 1` | `gcad2glb::export_height_flag_matches_gcad2glb_and_the_library`, `gcad2glb::export_scale_flag_fits_a_solid_part` |
| inspect close-ups, wireframe and stroke preview | supported | `gcad inspect f.gcad out.png --zoom x,y,z,r --wire --path x,y,z --path x,y,z` | `inspect_view::zoom_frames_a_region_and_wire_draws_the_triangles`, `inspect_view::stroke_path_overlay_lands_on_the_surface` |
| pick curvature | supported | `--pick x,y` prints curvature and bump/hollow/flat | `inspect_view::pick_reports_the_curvature_of_the_surface` |

## Flat solids in gcad-mesh

`gcad_mesh::build` runs these without the kernel: polygon triangulation and straight extrusion,
flat-shaded, rim fillets as bevel rings. Each prop in `tests/fixtures/props` is checked against
the kernel build.

| operation | status | syntax | tests |
|---|---|---|---|
| world plane | supported | `plane XZ offset=-t/2` | `solid_props::coin_matches_kernel`, `solid_props::negative_extrude_goes_below_the_plane` |
| mesh rect with rounded corners | supported | `rect 26 26 at=10,0 r=4` | `solid_props::barrier_matches_kernel`, `solid_props::repro_builds_a_closed_flat_shaded_part`, `solid_silhouette::barrier_silhouette_matches_kernel` |
| mesh circle | supported | `circle 18 at=60,0` | `solid_props::gate_matches_kernel`, `solid_silhouette::coin_silhouette_matches_kernel` |
| mesh poly | supported | `poly 0,0 10,0 5,8` | `solid_props::hurdle_matches_kernel`, `solid_silhouette::gate_silhouette_matches_kernel` |
| profile array | supported | `array count=4 step=180,0` | `solid_props::buildings_match_kernel`, `solid_silhouette::building_silhouettes_match_kernel` |
| profile inside a profile is a hole | supported | `rect 40 40` then `circle 10` | `solid_props::profile_inside_another_is_a_hole`, `solid_props::arch_matches_kernel` |
| mesh ellipse, ngon | supported | `ellipse 4 2`, `ngon 2 6 angle=30` | `shapes::an_ellipse_extrudes_to_its_area`, `shapes::an_ngon_extrudes_into_a_prism_of_that_many_sides` |
| mesh torus | supported | `rim: torus 4 1 around=32 sides=12` | `shapes::a_torus_has_the_volume_of_a_ring_and_its_own_part`, `shapes::a_torus_lies_in_its_plane_at_its_offset`, `shapes::a_torus_tube_must_be_thinner_than_its_ring` |
| mesh extrude to a frustum or cone | supported | `extrude 3 scale=0.5`, `extrude 3 scale=0` | `shapes::extrude_scale_makes_a_frustum_and_zero_makes_a_cone` |
| low-poly shell | supported | `eye: blob 0,0,0 r=1,1,0.5 mode=shell segments=12` | `shapes::a_shell_blob_with_segments_is_a_low_poly_ellipsoid`, `shapes::segments_need_a_shell`, `shapes::low_poly_shells_keep_their_part_names_and_colours` |
| mesh extrude | supported | `extrude 8`, `extrude -4` | `solid_props::train_matches_kernel`, `solid_props::road_matches_kernel`, `solid_silhouette::road_silhouette_matches_kernel` |
| mesh repeat | supported | `repeat frame count=2 step=0,250` | `solid_silhouette::train_silhouette_matches_kernel`, `solid_silhouette::hurdle_silhouette_matches_kernel` |
| mesh extrude rim fillet | supported | `fillet 2 foot.end&foot.side` | `solid_silhouette::fillet_rim_stays_close_to_the_kernel_arc`, `solid_silhouette::coin_face_normals_match_kernel`, `solid_silhouette::barrier_face_normals_match_kernel`, `solid_silhouette::arch_silhouette_matches_kernel` |
| solid props build fast and small | supported | `gcad_mesh::build` on a prop file | `solid_silhouette::props_stay_within_five_times_the_kernel_glb_triangles`, `solid_silhouette::building_all_props_is_fast` |

## Assemblies

Assemblies are `.gasm` files. They bring in the bodies of `.gcad` part files and join them.

| operation | status | syntax | tests |
|---|---|---|---|
| bring in a part file | supported | `part box box.gcad body=lid` | `assembly::parts_from_files`, `assembly::kept_parts_follow_their_files`, `assembly::kept_parts_rebuild_only_what_changed`, `stored::stored_parts_follow_their_files`, `stored::stored_sub_assemblies_follow_their_parts`, `stored::stored_meshes_come_back_the_same`, `assembly::a_newer_build_stops_the_older_one` |
| set a part's variables | supported | `part plate plate.gcad w=size` | `assembly::part_variables` |
| geometry stays in part files | supported | `rect` in a `.gasm` fails | `assembly::geometry_stays_in_parts`, `assembly::part_errors_name_the_file` |
| concentric mate | supported | `concentric pin:pin.side plate:hole.side near=x,y,z` | `assembly::concentric_pin_in_a_hole`, `assembly::concentric_picks_the_hole_near_a_point`, `assembly::concentric_far_from_the_origin` |
| flush mate | supported | `flush pin:pin.start plate:base.end offset=1` | `assembly::concentric_pin_in_a_hole` |
| parallel mate | supported | `parallel b:rod.end a:rod.end` | `assembly::parallel_and_angle_turn_parts`, `assembly::a_parallel_mate_stops_a_joint` |
| angle mate | supported | `angle lid:plate.end base:plate.end 30` | `assembly::parallel_and_angle_turn_parts` |
| distance mate | supported | `distance b:rod.side a:rod.side 50` | `assembly::distance_between_axes` |
| tangent mate | supported | `tangent rod:rod.side plate:plate.end` | `assembly::tangent_lays_a_rod_on_a_plate` |
| mates hold parts together | supported | a screw mated to lid and box stops the hinge | `assembly::a_screw_locks_the_hinge` |
| sub-assemblies | supported | `part kit boxed.gasm` keeps its joints, mates and axes; `move kit 50,0,0` moves it whole | `assembly::a_sub_assembly_keeps_its_mates`, `assembly::a_sub_assembly_moves_as_one`, `assembly::sub_assemblies_build_a_shared_part_once`, `assembly::assemblies_that_hold_each_other_fail` |
| check holes line up | supported | `aligned lid:screws.side box:pilot.side tol=0.05` | `assembly::holes_line_up` |
| turning joint | supported | `joint open box.lid box.main turn about=hinge at=-90` | `assembly::hinge_opens_the_lid` |
| sliding joint | supported | `joint pull chest.drawer chest.main slide along=0,-1,0 max=30` | `assembly::slide_moves_a_drawer` |
| cylindrical joint | supported | a `turn` and a `slide` on one axis | `assembly::a_rod_turns_and_slides_on_one_axis` |
| closed loops of joints | supported | a four-bar linkage follows its crank | `assembly::a_four_bar_linkage_follows_its_crank` |
| gears and racks | supported | `couple jb ja ratio=-0.5` | `assembly::coupled_joints_move_together`, `assembly::couple_losses_are_checked`, `assembly::gears_in_mesh_turn_without_touching`, `assembly::planetary_gears_turn_without_touching` |
| motors driving joints through gears | supported | `examples/robot/arm.gasm` | `assembly::robot_arm_example` |
| posing a joint | supported | `pose open -90` | `assembly::pose_moves_children` |
| check every pair of parts | supported | `interference none` | `assembly::clear_assembly`, `assembly::strict_interference_fails` |
| meshing gears may touch | supported | `couple ... module=2 gear=a pinion=b`, overlap under `module`³ allowed | `assembly::meshing_gears_may_touch` |
| pattern parts | supported | `pattern s1 holes=case.lid:screws.side`, `count=` `step=`/`angle=` | `assembly::pattern_copies_a_part_and_its_mates` |
| exploded view | supported | `explode lid 0,0,30`, `render --explode` | `assembly::exploded_views_move_parts_apart` |
| assembly drawing with parts list | supported | `gcad export top.gasm top.svg` | `output::assembly_drawing_lists_parts` |
| bill of materials | supported | `gcad bom top.gasm [--csv]` | `assembly::bill_of_materials_counts_parts` |
| sweep a joint for clashes | supported | `interference joint=open steps=8` | `assembly::sweep_finds_a_clash`, `assembly::sweep_keeps_reporting_parts_that_do_not_move` |
| standard screws, nuts, washers and inserts | supported | `part s1 shcs:M3x10`, `nut:M3`, `washer:M3`, `insert:M3` | `assembly::standard_parts_from_the_library` |
| bearings, dowels and standoffs | supported | `part b bearing:608`, `dowel:4x16`, `standoff:M3x10` | `assembly::bearings_dowels_and_standoffs` |
| vendor STEP assembly as parts | supported | `part box vendor.step` | `assembly::step_assembly_as_a_part` |
| STEP length units and board exports | supported | `part pcb board.step` with inch parts, faceted breps and surface models | `assembly::step_part_in_its_own_units`, `assembly::kicad_board_in_inches_and_surface_models`, `assembly::step_voids_outside_the_solid_are_parts` |

## Output

| operation | status | syntax | tests |
|---|---|---|---|
| STEP export | supported | `gcad export part.gcad part.step` | `output::step`, `output::step_opens_in_opencascade`, `output::assembly_step_keeps_multi_shell_parts` |
| STL export | supported | `gcad export part.gcad part.stl` | `output::stl` |
| simulation parts export | supported | `gcad export wind.gsim parts.stl` writes the parts the simulation keeps after `leave` | `output::simulation_parts_export` |
| PNG views | supported | `gcad render part.gcad part.png` | `output::png` |
| turntable and pose sheets | supported | `gcad render f.gcad sheet.png --turns 8 --clip walk --times 0,0.25,0.5`: views round the model, one row per clip time, framed the same in every cell | `output::turntable_sheet_has_a_cell_per_angle_and_pose` |
| STEP import | supported | `import part.step`, `import vendor.step solid=2` | `output::step_import`, `output::step_import_from_opencascade`, `output::step_import_asks_which_solid`, `output::step_import_boxes_what_it_cannot_read`, `output::step_import_reads_a_revolved_circle`, `output::step_import_leaves_unreadable_cavities_solid` |
| STEP instances placed | supported | `import rig.step solid=4` puts a repeated part where the file places it | `output::step_import_places_instances` |
| OBJ export | supported | `gcad export part.gcad part.obj` | `output::obj` |
| 3MF export | supported | `gcad export part.gcad part.3mf` | `output::three_mf`, `output::three_mf_names_the_bodies` |
| SVG drawing | supported | `gcad export part.gcad part.svg` | `output::drawing` |
| drawing dimensions and hole callouts | supported | overall sizes, `4× ⌀3.2` | `output::drawing_dimensions_and_section` |
| drawing labels clear of the view | supported | dimension and callout labels do not overlap the part | `output::drawing_labels_sit_clear_of_the_view` |
| section view | supported | `gcad export part.gcad part.svg --section y=0` | `output::drawing_dimensions_and_section` |
| STL import | supported | `import part.stl` | `output::stl_import` |
| drawing sheets | supported | `.gdwg` file: `sheet`, `view`, `section`, `dims`, `dim`, `level`, `callout`, `note`, `cutlist`, exported with `gcad export x.gdwg x.pdf` | `output::drawing_sheets`, `output::drawing_view_that_does_not_fit` |
| panel nesting | supported | `nest panels_* stock=2400x600 gap=4` in a `.gdwg`, or `gcad nest file.gasm --stock 2400x600 --gap 4`: packs flat parts onto stock with guillotine cuts, picks the least-waste mix of stock sizes, keeps showing edges on factory-edged sides, reports waste and lists the cuts in order | `output::drawing_nests_panels_on_stock` |
| STEP colours | supported | `color red` | `output::step_colours` |
| see-through bodies in the viewer | supported | `color #9cc7d9 opacity=0.3`, `color glass red opacity=0.5` | `assembly::see_through_parts` |
| STEP assembly structure | supported | `gcad export top.gasm top.step` | `output::step_assembly`, `output::step_assembly_opens_in_opencascade` |


## Parameters and inspection

| operation | status | syntax | tests |
|---|---|---|---|
| variables and expressions | supported | `let w=40 h=w*3/4` | `inspect::variables` |
| math functions | supported | `let a=atan2(3,4) r=sqrt(x)` | `inspect::functions` |
| face and edge selectors | supported | `label.group`, `>Z`, `+X`, `all`, `a,b`, `a&b`, `x\|y` | `inspect::groups_and_edges`, `inspect::directional_faces` |
| groups survive later edits | supported | | `inspect::groups_survive_later_cuts` |
| errors that say what to do | supported | | `inspect::errors` |
| measure distance | supported | `measure faces faces` | `inspect::measure_distance` |
| measure an angle | supported | `measure angle faces faces` | `inspect::measure_angle` |
| mass properties | supported | `measure mass` | `inspect::mass_properties` |
| materials and mass | supported | `material steel`, `material pla density=1.24` | `inspect::material_mass`, `assembly::materials_follow_parts` |
| wall thickness | supported | `measure thickness` | `inspect::wall_thickness` |
| draft analysis | supported | `measure draft pull=z` | `inspect::draft_analysis` |
| bounding boxes | supported | `measure box`, `measure box pcb base.end` | `inspect::measure_boxes` |
| clearance between bodies | supported | `measure clearance pcb lid` | `inspect::measure_clearance`, `assembly::clearance_between_parts` |
| free space inside a case | supported | `measure space at=0,0,10`, `gcad measure case.gasm space` | `inspect::measure_space`, `assembly::clearance_between_parts` |
| variables from outside the file | supported | `gcad check part.gcad --set w=20` | `inspect::outside_variables` |
| include another file | supported | `include part.gcad d=10`; its new `let` names carry over | `inspect::include_file`, `inspect::include_brings_in_let` |
| conditional lines | supported | `if w>30 chamfer 1 edges` | `inspect::conditional` |

## Simulation

The plan for the rest is in `docs/simulation.md`.

| operation | status | syntax | tests |
|---|---|---|---|
| joint torque against gravity | supported | `sim torque`, `part arm.gasm`, `sweep shoulder elbow steps=24` | `simulation::torque_of_a_pendulum`, `simulation::torque_needs_joints` |
| torque through gears and racks | supported | a `couple` carries the load to the joint it drives | `simulation::torque_through_gears`, `simulation::robot_arm_motor_torque` |
| gear and worm losses | supported | `couple ... efficiency=0.9 back=0.8`, `couple ... lead=3.6 friction=0.1` | `simulation::torque_through_lossy_gears`, `simulation::worm_that_locks` |
| torque on a joint | supported | `torque az 6.3` in `sim torque` | `simulation::torque_on_a_joint` |
| gear mesh forces, stresses, power and heat | supported | `couple ... module=1 face=10 gear=wheel pinion=worm`, `speed az 1` in `sim torque` | `simulation::worm_drive_forces_stresses_and_heat`, `simulation::spur_gear_mesh_forces`, `simulation::couple_geometry_needs_module_and_face` |
| joint friction | supported | `joint ... drag=0.05` | `simulation::drag_on_joints` |
| payloads | supported | `payload hand 300 at=x,y,z` | `simulation::torque_of_a_pendulum`, `simulation::payload_follows_the_pose` |
| expectations | supported | `expect shoulder<2 drive>0.5` | `simulation::torque_through_gears` |
| stale results and charts | supported | `gcad check x.gsim`, `gcad render x.gsim x.png` | `simulation::result_goes_stale` |
| readings and sweeps in the viewer | supported | the simulation card lists readings and checks, traces each sweep, and runs the file | `simulation::torque_readings_without_a_field` |
| progress while a simulation runs | supported | `gcad sim x.gsim` prints each stage to stderr (`-q` for none); the viewer's simulation card shows bars while it runs | `progress::static_run_reports_its_stages` |
| setup before a result | supported | the viewer draws the mesh with its loads and supports, or the flow parts, duct, wind and heated faces | `simulation::setup_shown_before_a_result`, `simulation::flow_setup_shown_before_a_result` |
| material libraries | supported | `materials.toml` beside or above a file, `~/.config/gcad/materials.toml`, `gcad materials` | `simulation::material_library_is_found` |
| how a material looks | supported | `color`, `roughness`, `metallic`, `clearcoat`, `transmission`, `ior`, `emission` in `materials.toml`, for the viewer and its ray tracer | `simulation::material_library_sets_how_a_body_looks` |
| standard material library | supported | `material stainless-316`, `material c24`, `material pir`; `gcad materials` lists them all | `simulation::standard_library_covers_common_materials`, `simulation::standard_library_covers_construction_and_printing` |
| complete standard materials | supported | every standard material has density, `E`, `nu`, `yield`, `k`, `alpha`, `cp`, colour, roughness, metallic and a texture | `simulation::every_standard_material_is_complete` |
| specific heat | supported | `cp = 900` in `materials.toml`, `material steel cp=500` | `simulation::specific_heat_is_a_material_property` |
| material textures | supported | `texture = "wood"`, `texture_scale`, `texture_rotation`, `color_map`, `normal_map`, `roughness_map`, `metallic_map` in `materials.toml` | `simulation::project_library_inherits_and_textures` |
| material files from other tools | supported | `materials workbench.xml`, `materials PLA.FCMat` | `simulation::material_files_import` |
| materials along three axes | supported | `E1= E2= E3= Xt= Zt=` in `materials.toml`, simulated with the weakest | `simulation::directional_material_takes_the_weakest` |
| stiffness, strength and conductivity | supported | `material pla`, `material delrin density=1.41 E=3.1 nu=0.35 yield=65` | `simulation::material_properties` |
| tetrahedral mesh | supported | `mesh 2`, `gcad sim x.gsim --dry-run`, `gcad render x.gsim x.png` | `simulation::tet_mesh_of_a_box`, `simulation::tet_mesh_of_curved_parts`, `simulation::tet_mesh_of_example_parts`, `simulation::tet_mesh_of_thin_walls`, `simulation::tet_mesh_of_a_ring_gear_part`, `simulation::mesh_dry_run`, `simulation::revolved_profile_on_its_axis_meshes`, `simulation::tet_mesh_of_stacked_cylinders`, `simulation::tet_mesh_of_a_bore_with_a_tiny_step`, `simulation::slightly_flat_elements_keep_the_mesh_size` |
| static stress | supported | `sim static`, `fix faces`, `load faces force=x,y,z`; `stress` leaves out the band around supports | `simulation::bar_in_tension`, `simulation::cantilever_stress`, `simulation::peak_stress_leaves_out_the_fixed_edge`, `simulation::small_part_keeps_its_own_stress_band`, `simulation::static_needs_supports_and_stiffness` |
| sliding supports and pressure | supported | `slide <X`, `pressure h.side 10` | `simulation::bar_in_tension`, `simulation::thick_cylinder_under_pressure` |
| pins and torques | supported | `hinge: pin h.side`, `torque rod.end 10` | `simulation::shaft_in_torsion`, `simulation::torque_about_a_named_axis`, `simulation::pinned_link` |
| bearings | supported | `left: bearing seat.side axial`, `right: bearing seat2.side` | `simulation::bearings_carry_force_and_no_moment`, `simulation::bearings_need_one_held_along_the_axis` |
| bearings between parts | supported | `bearing kfl:bore.side spindle axial C0=2900` | `simulation::bearing_between_parts_carries_no_moment`, `simulation::bearing_between_parts_takes_thrust`, `simulation::bearing_row_must_touch_the_shaft` |
| leaving parts out | supported | `leave ctl* *screw*` | `simulation::leave_parts_out` |
| mesh convergence check | supported | `converge`, `converge within=2` | `simulation::mesh_convergence_check` |
| probes | supported | `top: probe 100,0,10`; a point on a curved face just outside the mesh reads on its surface | `simulation::cantilever_stress`, `simulation::thick_cylinder_under_pressure`, `simulation::probe_on_a_curved_face_snaps_to_the_mesh` |
| example simulations | supported | `examples/parts/bracket.gsim`, `examples/parts/heatsink.gsim` | `simulation::example_simulations` |
| large problems | supported | conjugate gradients with a two-level preconditioner, above 100,000 unknowns for stress and 150,000 for heat; LOBPCG above 250,000 for frequencies and buckling | `simulation::large_problems_iterate`, `simulation::large_heat_problem_solves_iteratively`, `simulation::large_eigenproblems_iterate` |
| stress plots | supported | `gcad render x.gsim x.png`; the viewer reads the value, movement and point under the cursor, and draws and lists every load and support with its total | `simulation::stress_field_render`, `simulation::field_reads_at_a_point` |
| steady heat | supported | `sim thermal`, `cold: temperature faces 20`, `heat faces 5`, `source 5` | `simulation::conduction_through_a_wall`, `simulation::heat_needs_a_boundary` |
| convection | supported | `air: convect faces h=20 ambient=20` | `simulation::fin_with_convection` |
| thermal expansion | supported | `warm 50`, `warm -100 part=bolt`, `warm heat.gsim from=20` | `simulation::thermal_expansion`, `simulation::expansion_from_a_heat_result`, `simulation::preload_by_cooling_one_part` |
| buckling | supported | `sim buckling`, `load >Z force=0,0,-1000` | `simulation::column_buckling` |
| bonded assemblies | supported | `part stack.gasm`, `fix a:<Z`, `load b:>Z force=0,0,1000` | `simulation::bonded_parts_carry_load`, `simulation::bonded_joint_carries_moment`, `simulation::large_bonded_problem_converges`, `simulation::floating_part_is_refused`, `simulation::bonded_parts_conduct_heat`, `simulation::bonded_parts_vibrate_as_one` |
| keeping parts apart | supported | `apart cover* wheel*` | `simulation::parts_kept_apart_do_not_bond_across_a_clearance` |
| contact | supported | `contact a b` | `simulation::contact_presses_but_lets_go`, `simulation::contact_vibrates_as_bonded` |
| friction | supported | `contact a b friction=0.3` | `simulation::friction_holds_until_it_slides` |
| natural frequencies | supported | `sim modal`, `modes 3` | `simulation::cantilever_modes`, `simulation::free_beam_modes` |
| wind loads | supported | `sim flow`, `wind 30 toward=1,0,0 about=0,0,0`, `mesh 5`, `average 40`, `precision half`, `air density=1.2 viscosity=1.5e-5` | `simulation::wind_on_a_cube`, `simulation::wind_in_half_precision` |
| wind pressure in a static simulation | supported | `wind flow.gsim` | `simulation::wind_loads_a_static_part` |
| air sealed inside parts | supported | a closed box in a `sim flow` is airtight even with walls thinner than a cell | `simulation::wind_seals_a_closed_box` |
| flow slices, streamlines and playback | supported | `gcad render wind.gsim flow.png --flow`, or `--lines` for 3D streamlines; the viewer draws 3D streamlines, particles along them and the slices moving through time, from `wind.frames.bin` | `simulation::wind_draws_the_flow` |
| heat carried off by the air | supported | `heat <Z 10` in a `sim flow`, `air temperature=25`, `duct 40 25`; reads the heated faces' temperature, the hottest part and the resistance in K/W; a spinning fan cools parts that stand still | `simulation::fan_cools_a_ducted_heatsink`, `simulation::flow_heat_lines_parse`, `simulation::a_spinning_fan_cools_a_heated_post` |
| fans and propellers | supported | `spin spin rpm=3000` in a `sim flow` turns the joint's part through the lattice; reads `spin.torque`, `spin.power`, `spin.thrust`, `spin.flow` | `simulation::a_spinning_fan_pushes_air`, `simulation::spin_needs_a_turn_joint_in_a_flow` |
| motor torque, cogging and back-EMF | supported | `sim magnetic`, `turn spin sweep=6 steps=12`, `magnet`, `iron material=m19`, `coil turns= current=` on a 2D slice | `simulation::a_wire_turns_a_magnet_from_parts`, `simulation::magnetic_lines_need_their_simulation` |
| beam members from solids | supported | `sim frame`, each body a straight prismatic member, `fix`, `pin`, `slide` on end faces, `load` on end or side faces, `gravity` | `simulation::frame_cantilever_matches_beam_theory`, `simulation::frame_beam_on_pins_carries_a_uniform_load`, `simulation::frame_portal_sways_as_slope_deflection_says`, `simulation::frame_joins_a_girder_to_the_middle_of_a_column`, `simulation::frame_members_may_slope`, `simulation::frame_asks_for_what_it_needs` |
| member end releases | supported | `release beam:<X` | `simulation::frame_releases_turn_a_fixed_beam_into_a_simple_one` |
| load combinations | supported | `combine uls weight:1.35 live:1.5`, readings `uls.stress`, `uls.base` | `simulation::frame_combinations_add_loads_by_their_factors` |
| frame modes and buckling | supported | `modes 3`, `buckling 2` in a `sim frame` | `simulation::frame_cantilever_rings_at_its_beam_frequency`, `simulation::frame_columns_buckle_at_eulers_load` |
| frame stress and deflection drawn | supported | `gcad render portal.gsim portal.png`, the viewer's simulation card | `simulation::frame_view_shows_the_deflected_members` |
| thermal bridges and wall U-values | supported | `sim conduction`, `convect` with one label per side, readings `coupling`, `L.a.b`, `fRsi`, `coolest` | `simulation::conduction_through_a_composite_wall`, `simulation::a_steel_pin_bridges_the_insulation`, `simulation::conduction_through_a_round_part`, `simulation::conduction_asks_for_what_it_needs` |
| room acoustics | supported | `sim acoustics`, `absorb all plaster`, `speaker x,y,z`, `hall: probe x,y,z`, readings `hall.T30`, `hall.EDT`, `hall.C80`, `hall.D50`, `hall.STI` | `simulation::shoebox_rings_between_eyring_and_sabine`, `simulation::absorption_shortens_the_reverberation`, `simulation::obstacles_take_volume_out_of_the_room`, `simulation::acoustics_needs_absorption_and_a_source`, `simulation::acoustic_view_draws_the_room` |
| example building simulations | supported | `examples/frame/portal.gsim`, `examples/conduction/tie.gsim`, `examples/acoustics/room.gsim` | `simulation::building_example_simulations` |
| building simulations on remote runners | supported | `gcad sim x.gsim --remote` packs frame and acoustics jobs like any other | `simulation::building_kinds_run_remotely`, `simulation::new_kinds_have_no_mesh_to_converge` |
| wind on an assembly | supported | `part pair.gasm` in a `sim flow` reads `front.drag`, `back.drag` | `simulation::wind_shelters_a_part_behind` |

## Building datums and walls

Architecture stage A (gcad #348). The design is in `docs/architecture.md`; the tests are in `tests/arch.rs`.

| operation | status | syntax | tests |
|---|---|---|---|
| units and building header | missing | `units mm`, `building "Name" north=20 datum=0` | `arch::units_and_building_header` |
| levels | missing | `level L1 2700 "First"`, then `base=L0 top=L1-300` anywhere a height goes | `arch::levels_are_heights` |
| grids | missing | `grid A x=0`, `grid 1 y=0`, then `A1`, `A1+0,500` anywhere a point goes | `arch::grids_are_coordinates` |
| grids from a file | missing | `gridfile grids.csv` | `arch::grids_from_a_file` |
| layered wall types | missing | `wall_type ext brick:100 pir:100 block:100` (outside to inside, materials from the library), one body per layer `w1.brick` | `arch::wall_is_its_layers` |
| straight walls | missing | `w1: wall x,y x,y type=ext base=L0 top=L1 justify=outside\|centre\|inside offset=-150` | `arch::wall_justification_and_offset` |
| closed wall loops | missing | `wall A1 B1 B2 A2 closed type=ext` | `arch::closed_wall_loop` |
| polyline and arc walls | missing | `wall x,y x,y x,y`, `wall arc x,y x,y x,y` | `arch::polyline_and_arc_walls` |
| walls from a sketch | missing | `rect ...` then `wall profile type=` | `arch::wall_from_a_sketch_profile` |
| sloped and battered walls | missing | `top_offset=0,600`, `batter=1` | `arch::sloped_and_battered_walls` |
| pilasters | missing | `pilaster=300x200@2000` | `arch::wall_with_pilasters` |
| automatic wall joins | missing | walls that meet join (L, T, cross) with no gap or overlap | `arch::tee_join_has_no_gap_or_overlap` |
| wall join overrides | missing | `wall_join w1 w2 mitre\|butt\|tee` | `arch::wall_join_overrides` |
| stacked walls | missing | `type=ext:0..900,int:900..L1` | `arch::stacked_wall` |
| wall sweeps | missing | `wall_sweep w1.inside rect 15x100 at=0` (skirting, cornice, coping) | `arch::wall_sweep_runs_along_the_wall` |
| wall cuts | missing | `wall_cut w1 rect 1000 500 at=2000,2450` | `arch::wall_cut_by_a_profile` |
| wall faces as selectors | missing | `w1.outside`, `w1.inside`, `w1.top`, `w1.ends` | `arch::wall_faces_are_selectors` |

## Doors and windows

Architecture stage A (gcad #348). An opening is hosted by a wall: placed by `at=`, the distance along the wall to the opening's centre, it cuts every layer and moves with the wall.

| operation | status | syntax | tests |
|---|---|---|---|
| hosted openings | missing | `o1: opening on=w1 at=1200 w=900 h=2100 sill=0` | `arch::opening_cuts_every_layer` |
| openings move with their wall | missing | move the wall's points and the opening follows | `arch::moving_a_wall_moves_its_openings` |
| opening checks | missing | an opening past the wall's end or top, or over another, fails | `arch::openings_that_leave_the_wall_or_overlap_fail` |
| doors | missing | `d1: door on=w1 at=1200 w=900 h=2100 hinge=left in\|out frame=70x45 leaf=44 threshold=0 mark=D01`, bodies `d1.frame`, `d1.leaf` | `arch::door_has_frame_leaf_and_mark` |
| door swings | missing | `swing=left\|right\|double\|slide\|fold` | `arch::door_swings` |
| windows | missing | `win1: window on=w1 at=3000 w=1200 h=1200 sill=900 panes=2x1 frame=60 mullion=40 glazing=double\|triple mark=W01`, glass as its own bodies | `arch::window_panes_and_glass` |
| window heads | missing | `head=flat\|arch\|segment\|round` | `arch::window_head_shapes` |
| garage doors | missing | `garage_door on=w1 at= w= h= panels=4` | `arch::garage_door` |
| library openings as parts | missing | `door on=w1 ... part=flush.gcad`, the file's `w`, `h` set from the line | `arch::door_from_a_part_file` |

## Floors and roofs

Architecture stage B (gcad #349).

| operation | status | syntax | tests |
|---|---|---|---|
| slabs and slab openings | missing | `s1: slab L1 A1 B1 B2 A2 type=concrete:200`, `slab_opening s1 x,y ...` | `arch::slab_with_an_opening` |
| layered slabs | missing | `type=concrete:200+screed:50+tile:10`, top of the last layer on the level | `arch::layered_slab_is_its_layers` |
| sloped slabs | missing | `slope=0.02 falls_to=x,y` | `arch::sloped_slab_falls_to_a_point` |
| ceilings | missing | `c1: ceiling room=hall h=2400 type=gypsum:13` | `arch::ceiling_in_a_room` |
| floor finishes | missing | `f1: floor_finish room=hall type=oak:20` | `arch::floor_finish_in_a_room` |
| foundations | missing | `foundation strip outer w=600 d=900`, `foundation pad x,y w= l= d=` | `arch::strip_and_pad_foundations` |
| ground slabs | missing | `ground_slab outer type=concrete:150+pir:100` | `arch::ground_slab_inside_the_walls` |
| gable roofs | missing | `r1: roof outer type=gable pitch=45 overhang=450 thickness=200 ridge=x base=L1` | `arch::gable_roof_ridge_and_area` |
| roof types | missing | `type=hip\|half_hip\|shed\|flat\|mansard\|gambrel\|barrel` | `arch::roof_types` |
| per-edge pitch | missing | `pitch_edge r1.eave.1 60` | `arch::per_edge_pitch` |
| walls trim to the roof | missing | `roof ... walls=outer` | `arch::walls_trim_to_the_roof_underside` |
| roof edges as selectors | missing | `r1.ridge`, `r1.hip`, `r1.valley`, `r1.eave`, `r1.verge` | `arch::roof_edges_are_selectors` |
| roof joins | missing | `roof_join r2 r1` | `arch::roof_join_meets_two_roofs` |
| roof by extrusion | missing | a profile, then `roof extrude 3000 thickness=200` | `arch::roof_extruded_from_a_profile` |
| dormers | missing | `dormer on=r1 at=x,y w=1200 type=gable` | `arch::dormer_on_a_roof` |
| skylights | missing | `skylight on=r1 at=x,y w=780 h=980` | `arch::skylight_on_a_roof` |
| chimneys | missing | `chimney x,y w=600 d=600 up_to=r1 over=900` | `arch::chimney_through_the_roof` |
| gutters and downpipes | missing | `gutter along=r1.eave.1 w=100 slope=0.005`, `downpipe from=gu.end d=68 to=L0` | `arch::gutter_and_downpipe` |
| parapets | missing | `parapet outer h=600 coping=50` | `arch::parapet_on_a_flat_roof` |

## Stairs, ramps and railings

Architecture stage B (gcad #349).

| operation | status | syntax | tests |
|---|---|---|---|
| stairs | missing | `st1: stair from=L0 to=L1 at=x,y dir=0 width=1000 rise=auto tread=auto rule=2R+T=630 max_rise=180`, risers and treads reported | `arch::straight_stair_follows_the_rule` |
| stair shapes | missing | `shape=straight\|L\|U\|winder\|spiral landing=900`, bodies `st1.flight.N`, `st1.landing.N` | `arch::stair_shapes` |
| stringers and nosing | missing | `stringer=closed\|open\|mono nosing=20` | `arch::stair_stringers_and_nosing` |
| stair checks | missing | too steep for the rule, headroom under 2000 | `arch::steep_stair_and_low_headroom_fail` |
| stair cut | missing | `stair_cut st1 s1 headroom=2000` | `arch::stair_cut_opens_the_slab` |
| ramps | missing | `ramp from=x,y,z to=x,y,z slope=1:12 width=1200 landing=1500 every=10000` | `arch::ramp_slope_and_landings` |
| ladders | missing | `ladder x,y,z x,y,z width=400 rungs=@300` | `arch::ladder_rungs` |
| railings | missing | `railing path x,y,z ... height=900 post=50@1500 baluster=12@120 top=round:40 material=steel` | `arch::railing_posts_and_balusters` |
| railings along a stair or slab edge | missing | `railing along=st1.edge` | `arch::railing_along_a_stair` |
| glass balustrades and handrails | missing | `railing ... glass=12 post=none`, `handrail path ... d=40 bracket=@1000` | `arch::glass_balustrade_and_handrail` |

## Building structure and facade

Architecture stage B (gcad #349). Profiles are the `member` sections.

| operation | status | syntax | tests |
|---|---|---|---|
| columns | missing | `c1: column A1 rect 300x300\|round 300\|<member section> base=L0 top=L1 material=` | `arch::columns_at_grid_crossings` |
| column grids | missing | `column_grid rect 200x200 base=L0 top=L1` | `arch::column_at_every_grid_crossing` |
| beams | missing | `b1: beam A1 B1 bar 200x400 z=L1 material=c24` (top of beam on `z`) | `arch::beam_between_grid_points` |
| joists | missing | `joist <outline> bar 47x200 spacing=400 dir=y z=L1` | `arch::joists_across_a_floor` |
| trusses | missing | `truss x,y,z x,y,z type=king\|queen\|fink pitch=30 member=bar 47x150 spacing=600 count=10` | `arch::truss_member_counts` |
| braces | missing | `brace A1 B1 rod 20 from=L0 to=L1 type=x` | `arch::brace_between_columns` |
| footings | missing | `footing c1 1200x1200x500` | `arch::footing_under_a_column` |
| curtain walls | missing | `curtainwall x,y x,y height=L1 grid=1500x900 mullion=50x100 glass=double door=cell:2,1` | `arch::curtain_wall_grid` |
| louvres and shading fins | missing | `louvre x,y x,y z=900..2100 blade=150x20@100 angle=45`, `shading_fin x,y x,y depth=400 t=20@500` | `arch::louvres_and_fins` |
| balconies | missing | `balcony on=outer at=4000 level=L1 w=3000 d=1200 slab=concrete:200 railing=1100` | `arch::balcony_slab_and_railing` |

## Rooms and schedules

Architecture stage C (gcad #350).

| operation | status | syntax | tests |
|---|---|---|---|
| rooms | missing | `kitchen: room at=x,y name="Kitchen" floor=tile ceiling=plaster wall=paint level=L0`, boundary from the walls, area, perimeter and volume | `arch::room_found_from_the_walls` |
| room checks | missing | a room whose walls do not close fails | `arch::unclosed_room_fails` |
| room separators | missing | `room_separator x,y x,y` | `arch::room_separator_splits_a_space` |
| net and gross areas | missing | `area_scheme gross\|net\|rentable`, `gcad schedule f rooms\|areas --csv` | `arch::net_and_gross_area` |
| zones | missing | `zone day hall living` | `arch::zones_sum_their_rooms` |
| rooms on several levels | missing | `room ... level=L1` | `arch::rooms_on_two_levels` |
| door and window schedules | missing | `gcad schedule f doors\|windows --csv\|--json\|--svg` | `arch::door_and_window_schedule` |
| wall schedules | missing | `gcad schedule f walls`, gross and net area after openings | `arch::wall_schedule_nets_out_openings` |
| quantities by material and layer | missing | `gcad schedule f materials` | `arch::material_quantities_by_layer` |
| slab, roof, stair and column schedules | missing | `gcad schedule f slabs\|roofs\|stairs\|columns` | `arch::element_schedules` |
| finish schedules | missing | `gcad schedule f finishes` | `arch::finish_schedule` |
| cost columns | missing | `--rates rates.toml` (price per `m2`, `m3`, `m`, `item`) | `arch::cost_columns_from_rates` |
| stable schedules | missing | rows sorted by name, the same bytes every run | `arch::schedule_csv_is_sorted_and_stable` |
| schedules in drawings | missing | `schedule doors place=x,y` in a `.gdwg` | `arch::schedule_table_on_a_sheet` |
| building elements in the BOM | missing | `gcad bom house.gasm` lists layers and leaves | `arch::bom_lists_building_elements` |

## Architectural drawings

Architecture stage D (gcad #351). Lines of a `.gdwg` file.

| operation | status | syntax | tests |
|---|---|---|---|
| floor plans | missing | `plan L0 cut=1200 scale=1:50`: poche of cut walls and columns, door swings, window symbols | `arch::plan_draws_the_cut` |
| plan symbols | missing | `plan ... grids=1 levels=1`, `northarrow`, `scalebar` | `arch::plan_symbols` |
| stair arrows | missing | UP or DN text, arrow and break line on a plan | `arch::plan_stair_arrow` |
| reflected ceiling plans | missing | `rcp L0` | `arch::reflected_ceiling_plan` |
| elevations | missing | `elevation from=+y\|-y\|+x\|-x` with level marks | `arch::elevation_with_level_marks` |
| building sections | missing | `section a-a x,y x,y` with layer hatches | `arch::section_through_two_points` |
| material hatches | missing | `hatch brick pattern=brick` (ISO 128 table by material) | `arch::hatch_fill_area_matches_the_layer` |
| tags | missing | `tag door\|window\|wall\|stair\|column` | `arch::tags_carry_the_marks` |
| room tags | missing | `tag room`: name and area | `arch::room_tags_show_the_area` |
| automatic dimensions | missing | `dim auto walls\|openings\|grid side=-y` | `arch::auto_dims_sum_to_the_overall` |
| detail viewports | missing | `viewport window=u0,v0,u1,v1 scale=1:5 label=` | `arch::viewport_detail_at_a_new_scale` |
| sheet sets | missing | `sheet ... number=A-101`, `drawinglist` | `arch::sheet_set_with_a_drawing_list` |
| DXF plans with layers | missing | `gcad export plan.gdwg plan.dxf`, layers `A-WALL`, `A-DOOR`, `A-GLAZ`, `A-GRID` | `arch::plan_dxf_has_layers_by_class` |

## IFC

Architecture stage E (gcad #352).

| operation | status | syntax | tests |
|---|---|---|---|
| IFC4 export | missing | `gcad export house.gasm house.ifc`: project, site, building, storeys, walls, doors, windows, openings with voids and fills | `arch::ifc_has_the_spatial_structure` |
| IFC wall layers | missing | `IfcMaterialLayerSet` per wall type, `Pset_WallCommon` | `arch::ifc_wall_layers` |
| IFC element classes | missing | slabs, roofs, stairs, railings, columns, beams, spaces with quantities | `arch::ifc_element_classes` |
| stable GlobalIds | missing | ids derived from element names | `arch::ifc_global_ids_are_stable` |
| IFC syntax | missing | unique ids, balanced parentheses, every reference defined | `arch::ifc_step_syntax_is_valid` |
| IFC import | missing | `import model.ifc` as a read-only underlay | `arch::ifc_round_trip` |
| IFC checks | missing | `gcad check house.ifc` | `arch::ifc_check_finds_orphans` |
| image underlays | missing | `underlay plan.png scale=1:100 dpi=100` | `arch::image_underlay` |

## Site and analysis

Architecture stage F (gcad #353).

| operation | status | syntax | tests |
|---|---|---|---|
| terrain | missing | `t1: terrain points site.csv\|contour site.dxf\|grid 20000x20000 slope=0.1,0 depth=1000` | `arch::terrain_from_points` |
| pads and cut and fill | missing | `pad <outline> level=1000`, `cut_fill t1 p1` | `arch::cut_and_fill_of_a_pad` |
| site boundary | missing | `site_boundary x,y ... setback=3000` | `arch::site_boundary_area_and_setback` |
| roads, retaining walls and trees | missing | `road x,y x,y width=3500 type=asphalt:100`, `retaining x,y x,y h=1200 t=250`, `tree x,y h=6000 spread=4000` | `arch::site_elements` |
| U-values | missing | `check uvalue w1` (layer conductivities from the material library, Rsi 0.13, Rse 0.04) | `arch::u_value_of_a_wall` |
| heat loss | missing | `check heatloss dt=20` | `arch::heat_loss_estimate` |
| thermal bridges | missing | `check bridges` | `arch::thermal_bridge_at_a_slab_edge` |
| daylight | missing | `check daylight min=0.1` (glazing to floor area per room) | `arch::daylight_ratio` |
| stair and ramp rules | missing | `check stairs min_width=900`, `check ramps` | `arch::stair_and_ramp_rules` |
| door widths and turning circles | missing | `check doors clear=775 turning=1500` | `arch::door_clear_width_check` |
| egress, room and compartment checks | missing | `check egress max=`, `check rooms min_area= min_height=`, `check compartments max=` | `arch::egress_and_room_checks` |
| sun position | missing | `sun lat=51.5 date=21-06 time=12:00 solar` | `arch::sun_position` |
| shadow study | missing | `check shadow w1`, `gcad render --sun` | `arch::shadow_of_a_wall` |

## Buildings in gcad-mesh

Architecture stage G (gcad #354): the same lines build flat meshes with no kernel, for games.

| operation | status | syntax | tests |
|---|---|---|---|
| mesh walls and openings | missing | `wall`, `door`, `window` through `gcad_mesh::build` | `arch::mesh_wall_matches_the_kernel` |
| mesh house within budget | missing | walls, slab, roof, stair under 4000 triangles and 5 ms | `arch::mesh_house_within_budget` |
| door nodes | missing | `d1.leaf` a named body pivoting at its hinge | `arch::mesh_door_is_a_hinged_node` |
