mod common;

use common::*;
use gcad::mesh::{Mesh, P3, length, sub};
use std::sync::Arc;

const BALL: &str = "ball: blob 0,0,0 r=10\nremesh 0.25";

fn mesh_of(model: &gcad::model::Model) -> Arc<Mesh> {
    model.meshes().last().expect("a mesh body").1.clone()
}

fn moves(before: &Mesh, after: &Mesh) -> Vec<(P3, f64)> {
    assert_eq!(before.positions.len(), after.positions.len());
    before
        .positions
        .iter()
        .zip(&after.positions)
        .map(|(p, q)| (*p, length(sub(*q, *p))))
        .collect()
}

fn sculpt(lines: &str) -> (Arc<Mesh>, Arc<Mesh>) {
    let before = mesh_of(&build(BALL));
    let after = mesh_of(&build(&format!("{BALL}\n{lines}")));
    (before, after)
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "got {actual:.4}, expected {expected:.4} within {tolerance}"
    );
}

const TOP: P3 = [0.0, 0.0, 10.0];

fn closest_to(moved: &[(P3, f64)], at: P3, distance: f64) -> (f64, f64) {
    moved
        .iter()
        .map(|(p, m)| (length(sub(*p, at)), *m))
        .min_by(|a, b| (a.0 - distance).abs().total_cmp(&(b.0 - distance).abs()))
        .expect("vertices")
}

#[test]
fn dab_draw_raises_by_strength_with_a_linear_falloff() {
    let (before, after) =
        sculpt("brush b type=draw size=2 strength=0.5 falloff=linear\ndab b at=0,0,10");
    let moved = moves(&before, &after);
    for (p, m) in &moved {
        let d = length(sub(*p, TOP));
        if d < 2.0 {
            near(*m, 0.5 * (1.0 - d / 2.0), 0.01);
        }
    }
    let (d, m) = closest_to(&moved, TOP, 1.0);
    near(m, 0.5 * (1.0 - d / 2.0), 0.01);
    assert!(
        moved
            .iter()
            .all(|(p, m)| length(sub(*p, TOP)) < 2.05 || *m == 0.0),
        "a vertex outside the brush moved"
    );
}

#[test]
fn brush_edge_keeps_a_flat_core() {
    let (before, after) =
        sculpt("brush b type=draw size=2 strength=0.4 falloff=linear edge=0.5\ndab b at=0,0,10");
    let moved = moves(&before, &after);
    let (_, m) = closest_to(&moved, TOP, 0.8);
    near(m, 0.4, 0.01);
    let (d, m) = closest_to(&moved, TOP, 1.5);
    near(m, 0.4 * (1.0 - (d / 2.0 - 0.5) / 0.5), 0.03);
}

#[test]
fn stroke_lays_a_ridge_along_its_path() {
    let (before, after) = sculpt(
        "brush b type=draw size=1 strength=0.3 falloff=linear\nstroke b from=-3,0,10 to=3,0,10",
    );
    let moved = moves(&before, &after);
    for (p, m) in &moved {
        if p[2] < 9.0 {
            continue;
        }
        let along = p[0].abs() < 2.5;
        if along && p[1].abs() < 0.05 {
            near(*m, 0.3, 0.02);
        }
        if p[1].abs() > 1.05 {
            assert_eq!(*m, 0.0, "{p:?} is off the stroke");
        }
        if along && (p[1].abs() - 0.5).abs() < 0.03 {
            near(*m, 0.15, 0.03);
        }
    }
}

#[test]
fn stroke_pressure_ramps_the_ridge() {
    let (before, after) = sculpt(
        "brush b type=draw size=1 strength=0.3 falloff=flat\nstroke b from=-3,0,10 to=3,0,10 pressure=0,1",
    );
    let moved = moves(&before, &after);
    let (_, middle) = closest_to(&moved, [0.0, 0.0, 10.0], 0.0);
    near(middle, 0.15, 0.02);
}

#[test]
fn stroke_points_and_lazy_smooth_a_shaky_path() {
    let shaky = "-3,0,10 -2,0.6,10 -1,-0.6,10 0,0.6,10 1,-0.6,10 2,0.6,10 3,0,10";
    let reach = |lazy: &str| {
        let (before, after) = sculpt(&format!(
            "brush b type=draw size=0.5 strength=0.2 falloff=flat{lazy}\nstroke b {shaky}"
        ));
        moves(&before, &after)
            .iter()
            .filter(|(p, m)| *m > 0.0 && p[0].abs() < 1.5)
            .map(|(p, _)| p[1].abs())
            .fold(0.0, f64::max)
    };
    let (shaking, steady) = (reach("") - 0.5, reach(" lazy=0.8") - 0.5);
    assert!(
        shaking > 0.3 && steady < shaking / 2.0,
        "path wobble without lazy {shaking}, with lazy {steady}"
    );
}

#[test]
fn clay_builds_up_to_a_plane_above_the_surface() {
    let (_, after) = sculpt("brush c type=clay size=2 strength=0.2 falloff=flat\ndab c at=0,0,10");
    let top = after
        .positions
        .iter()
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    near(top, 10.2, 0.02);
    let low = after
        .positions
        .iter()
        .filter(|p| p[2] > 9.0 && (p[0] * p[0] + p[1] * p[1]).sqrt() < 1.2)
        .map(|p| p[2])
        .fold(f64::MAX, f64::min);
    near(low, 10.2, 0.02);
}

#[test]
fn move_brush_drags_a_region_by_the_stroke() {
    let (before, after) =
        sculpt("brush g type=move size=3 falloff=linear edge=0.5\nstroke g from=0,0,10 to=0,0,12");
    let moved = moves(&before, &after);
    let top = after
        .positions
        .iter()
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    near(top, 12.0, 0.03);
    let (d, m) = closest_to(&moved, TOP, 2.25);
    near(m, 2.0 * (1.0 - (d / 3.0 - 0.5) / 0.5), 0.05);
}

#[test]
fn snake_hook_pulls_out_a_spike() {
    let (_, after) = sculpt(
        "brush s type=snake_hook size=1 falloff=smooth detail=0.25\nstroke s from=0,0,10 to=0,0,14",
    );
    let top = after
        .positions
        .iter()
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    near(top, 14.0, 0.1);
    assert_eq!(after.open_edges(), 0);
}

#[test]
fn crease_brush_cuts_a_groove_of_its_strength() {
    let (_, after) =
        sculpt("brush d type=dam_standard size=0.6 strength=0.2\nstroke d from=-2,0,10 to=2,0,10");
    let deepest = after
        .positions
        .iter()
        .filter(|p| p[2] > 9.0 && p[0].abs() < 1.0)
        .map(|p| length(*p))
        .fold(f64::MAX, f64::min);
    near(10.0 - deepest, 0.2, 0.03);
}

#[test]
fn smooth_brush_calms_noise_only_under_it() {
    let rough = mesh_of(&build(&format!("{BALL}\nnoise 0.3 size=0.5")));
    let calm = mesh_of(&build(&format!(
        "{BALL}\nnoise 0.3 size=0.5\nbrush s type=smooth size=2 strength=4\ndab s at=0,0,10"
    )));
    let spread = |m: &Mesh| {
        let r: Vec<f64> = m
            .positions
            .iter()
            .filter(|p| length(sub(**p, TOP)) < 1.0)
            .map(|p| length(*p))
            .collect();
        let mean = r.iter().sum::<f64>() / r.len() as f64;
        r.iter().map(|v| (v - mean).abs()).fold(0.0, f64::max)
    };
    assert!(spread(&calm) < spread(&rough) * 0.5);
    for (p, q) in rough.positions.iter().zip(&calm.positions) {
        if length(sub(*p, TOP)) > 2.5 {
            assert_eq!(p, q);
        }
    }
}

#[test]
fn flatten_brush_planes_the_top() {
    let (_, after) = sculpt("brush f type=planar size=2 strength=1 falloff=flat\ndab f at=0,0,10");
    for p in &after.positions {
        if p[2] > 9.0 && (p[0] * p[0] + p[1] * p[1]).sqrt() < 1.5 {
            near(
                p[2],
                after
                    .positions
                    .iter()
                    .map(|p| p[2])
                    .fold(f64::MIN, f64::max),
                0.03,
            );
        }
    }
}

#[test]
fn pinch_brush_draws_vertices_to_the_stroke() {
    let (before, after) = sculpt(
        "brush p type=pinch size=1 strength=0.5 falloff=flat\nstroke p from=-2,0,10 to=2,0,10",
    );
    for (p, q) in before.positions.iter().zip(&after.positions) {
        if p[2] > 9.0 && p[0].abs() < 1.0 && p[1].abs() > 0.2 && p[1].abs() < 0.9 {
            near(q[1], p[1] * 0.5, 0.02);
        }
    }
}

#[test]
fn alpha_stamp_dents_and_raises_within_its_strength() {
    let (before, after) =
        sculpt("brush s type=stamp size=2 strength=0.1 alpha=pores(0.8,2)\ndab s at=0,0,10");
    let moved = moves(&before, &after);
    assert!(moved.iter().all(|(_, m)| *m <= 0.1 + 1.0e-9));
    let dents = before
        .positions
        .iter()
        .zip(&after.positions)
        .filter(|(p, q)| length(**q) < length(**p) - 0.005)
        .count();
    assert!(dents > 20, "{dents} dented vertices");
    let again = mesh_of(&build(&format!(
        "{BALL}\nbrush s type=stamp size=2 strength=0.1 alpha=pores(0.8,2)\ndab s at=0,0,10"
    )));
    assert_eq!(after.positions, again.positions);
}

#[test]
fn mirrored_brush_raises_both_sides_alike() {
    let (before, after) =
        sculpt("brush b type=draw size=1.5 strength=0.3 mirror=x\ndab b at=6,0,8");
    let moved = moves(&before, &after);
    let side = |s: f64| {
        moved
            .iter()
            .filter(|(p, _)| p[0] * s > 0.0)
            .map(|m| m.1)
            .fold(0.0, f64::max)
    };
    near(side(1.0), 0.3, 0.01);
    near(side(-1.0), 0.3, 0.01);
}

#[test]
fn geodesic_brush_stays_on_its_side_of_a_thin_shell() {
    let plate = "plate: blob 0,0,0 r=10,10,0.4\nremesh 0.1";
    let reach = |geodesic: &str| {
        let before = mesh_of(&build(plate));
        let after = mesh_of(&build(&format!(
            "{plate}\nbrush b type=draw size=1.5 strength=0.2{geodesic}\ndab b at=0,0,0.4"
        )));
        moves(&before, &after)
            .iter()
            .filter(|(p, m)| p[2] < 0.0 && *m > 0.0)
            .count()
    };
    assert!(reach("") > 0);
    assert_eq!(reach(" geodesic=on"), 0);
}

#[test]
fn masked_vertices_stay_still() {
    let (before, after) = sculpt("mask sphere=0,0,10 r=1.5\nselect all\ninflate 1");
    for (p, m) in moves(&before, &after) {
        let d = length(sub(p, TOP));
        if d < 1.45 {
            assert_eq!(m, 0.0, "{p:?}");
        } else if d > 1.55 {
            near(m, 1.0, 1.0e-6);
        }
    }
}

#[test]
fn brushes_respect_the_mask_and_invert_and_clear_it() {
    let (before, after) = sculpt(
        "mask sphere=0,0,10 r=1.5\nmask invert\nbrush b type=draw size=3 strength=0.3 falloff=flat\ndab b at=0,0,10",
    );
    for (p, m) in moves(&before, &after) {
        if length(sub(p, TOP)) > 1.55 {
            assert_eq!(m, 0.0);
        }
    }
    let (before, after) = sculpt("mask sphere=0,0,10 r=1.5\nmask clear\nselect all\ninflate 1");
    assert!(
        moves(&before, &after)
            .iter()
            .all(|(_, m)| (m - 1.0).abs() < 1.0e-6)
    );
}

#[test]
fn mask_by_height_ramps_between_two_levels() {
    let (before, after) = sculpt("mask by=height axis=z from=4 to=6\nselect all\ninflate 1");
    for (p, m) in moves(&before, &after) {
        let t = ((p[2] - 4.0) / 2.0).clamp(0.0, 1.0);
        near(m, 1.0 - t * t * (3.0 - 2.0 * t), 1.0e-6);
    }
}

#[test]
fn mask_grow_widens_and_shrink_narrows() {
    let reach = |extra: &str| {
        let model = build(&format!("{BALL}\nmask sphere=0,0,10 r=1{extra}"));
        let mesh = mesh_of(&model);
        (0..mesh.positions.len())
            .filter(|&i| mesh.mask[i] >= 0.5)
            .map(|i| length(sub(mesh.positions[i], TOP)))
            .fold(0.0, f64::max)
    };
    near(reach(""), 1.0, 0.15);
    near(reach("\nmask grow 1"), 2.0, 0.2);
    near(reach("\nmask grow 1\nmask shrink 0.5"), 1.5, 0.2);
    assert!(reach("\nmask blur 3") > 0.9);
}

#[test]
fn mask_by_cavity_finds_a_groove() {
    let model = build(&format!(
        "{BALL}\ncrease -3,0,10 3,0,10 depth=0.3 width=0.4\nmask by=cavity r=1"
    ));
    let mesh = mesh_of(&model);
    let masked: Vec<P3> = (0..mesh.positions.len())
        .filter(|&i| mesh.mask[i] >= 0.5)
        .map(|i| mesh.positions[i])
        .collect();
    assert!(masked.len() > 10, "{}", masked.len());
    assert!(masked.iter().all(|p| p[1].abs() < 0.6 && p[2] > 9.0));
}

#[test]
fn isolate_and_hide_keep_other_parts_still() {
    let two = "a: blob 0,0,0 r=3\nb: blob 10,0,0 r=3\nremesh 0.3";
    let before = mesh_of(&build(two));
    let after = mesh_of(&build(&format!("{two}\nisolate a\nselect all\ninflate 1")));
    for (i, (p, m)) in moves(&before, &after).into_iter().enumerate() {
        let part = &before.parts[before.labels[i] as usize];
        if part == "b" {
            assert_eq!(m, 0.0, "{p:?}");
        } else {
            near(m, 1.0, 1.0e-6);
        }
    }
    let after = mesh_of(&build(&format!(
        "{two}\nhide a\nshow\nselect all\ninflate 1"
    )));
    assert!(
        moves(&before, &after)
            .iter()
            .all(|(_, m)| (m - 1.0).abs() < 1.0e-6)
    );
    let error = failure(&format!("{two}\nhide c"));
    assert!(error.contains("a, b"), "{error}");
}

#[test]
fn group_makes_a_polygroup_to_select() {
    let model = build(&format!(
        "{BALL}\ngroup cap sphere=0,0,10 r=2\nselect cap\nmove 0,0,1"
    ));
    let mesh = mesh_of(&model);
    assert!(mesh.parts.contains(&"cap".to_string()));
    let top = mesh.positions.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    near(top, 11.0, 0.02);
}

#[test]
fn select_profile_and_strength_shape_the_falloff() {
    let (before, after) =
        sculpt("select sphere=0,0,10 r=0.3 falloff=2 profile=linear strength=0.5\ninflate 1");
    let moved = moves(&before, &after);
    let (d, m) = closest_to(&moved, TOP, 1.3);
    near(m, 0.5 * (1.0 - (d - 0.3) / 2.0), 0.01);
}

#[test]
fn join_softens_the_seam_between_parts() {
    let two = "a: blob 0,0,0 r=3\nb: blob 4,0,0 r=3\nremesh 0.2";
    let before = mesh_of(&build(two));
    let after = mesh_of(&build(&format!("{two}\njoin a b blend=1.5")));
    assert!(
        after.volume() > before.volume() * 1.005,
        "{} vs {}",
        after.volume(),
        before.volume()
    );
    for (p, q) in before.positions.iter().zip(&after.positions) {
        if (p[0] - 2.0).abs() > 2.5 {
            assert_eq!(p, q);
        }
    }
}

#[test]
fn dyntopo_refines_under_the_brush() {
    let coarse = "ball: blob 0,0,0 r=10\nremesh 1";
    let before = mesh_of(&build(coarse));
    let model = build(&format!(
        "{coarse}\ndyntopo on detail=0.2\nbrush b type=draw size=1.5 strength=0.3\ndab b at=0,0,10"
    ));
    let after = mesh_of(&model);
    assert!(after.triangles.len() > before.triangles.len());
    assert_eq!(after.open_edges(), 0);
    let longest = after
        .triangles
        .iter()
        .flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3])))
        .filter(|(a, b)| {
            length(sub(after.positions[*a as usize], TOP)) < 1.0
                && length(sub(after.positions[*b as usize], TOP)) < 1.0
        })
        .map(|(a, b)| {
            length(sub(
                after.positions[a as usize],
                after.positions[b as usize],
            ))
        })
        .fold(0.0, f64::max);
    assert!(longest <= 0.2 * 1.05, "{longest}");
    let top = after
        .positions
        .iter()
        .map(|p| length(*p))
        .fold(0.0, f64::max);
    near(top, 10.3, 0.05);
}

#[test]
fn subdivide_quadruples_the_triangles() {
    let before = mesh_of(&build(BALL));
    let after = mesh_of(&build(&format!("{BALL}\nsubdivide 1")));
    assert_eq!(after.triangles.len(), before.triangles.len() * 4);
    assert_eq!(after.open_edges(), 0);
}

#[test]
fn project_returns_a_selection_to_a_stored_level() {
    let model = build(&format!(
        "{BALL}\nlevel base\nsubdivide 1\nselect all\ninflate 1\nproject base"
    ));
    let worst = mesh_of(&model)
        .positions
        .iter()
        .map(|p| (length(*p) - 10.0).abs())
        .fold(0.0, f64::max);
    assert!(worst < 0.02, "{worst}");
}

#[test]
fn layer_amount_dials_its_strokes() {
    let base = mesh_of(&build(BALL));
    let lines =
        "layer bumps\nbrush b type=draw size=2 strength=0.5 falloff=linear\ndab b at=0,0,10";
    let most = |extra: &str| {
        let after = mesh_of(&build(&format!("{BALL}\n{lines}{extra}")));
        moves(&base, &after).iter().map(|m| m.1).fold(0.0, f64::max)
    };
    let full = most("");
    near(full, 0.5, 0.06);
    near(most("\nlayer bumps amount=0.5"), full * 0.5, 1.0e-6);
    near(most("\nlayer bumps amount=0"), 0.0, 1.0e-9);
    near(
        most("\nlayer bumps amount=0\nlayer bumps amount=2"),
        full * 2.0,
        1.0e-6,
    );
    let error = failure(&format!("{BALL}\nlayer bumps\nlayer other amount=1"));
    assert!(error.contains("bumps"), "{error}");
}

#[test]
fn finish_decimates_to_a_budget_and_reports_the_error() {
    let text = summary(&format!("{BALL}\nsubdivide 1\nfinish decimate=4000"));
    assert!(
        text.starts_with("4000 triangles") || text.starts_with("399"),
        "{text}"
    );
    assert!(text.contains("surface moved"), "{text}");
    let mesh = mesh_of(&build(&format!(
        "{BALL}\nsubdivide 1\nfinish decimate=4000"
    )));
    assert_eq!(mesh.open_edges(), 0);
    let worst = mesh
        .positions
        .iter()
        .map(|p| (length(*p) - 10.0).abs())
        .fold(0.0, f64::max);
    assert!(worst < 0.1, "{worst}");
}

#[test]
fn finish_to_an_error_bound_stays_within_it() {
    let full = mesh_of(&build(BALL));
    let model = build(&format!("{BALL}\nfinish max_error=0.02"));
    let done = mesh_of(&model);
    assert!(
        done.triangles.len() < full.triangles.len() / 3,
        "{}",
        done.triangles.len()
    );
    let (worst, _) = done.error_from(&full);
    assert!(worst <= 0.02, "{worst}");
    let bumpy = format!("{BALL}\nnoise 0.3 size=1");
    let coarse = mesh_of(&build(&format!("{bumpy}\nfinish max_error=0.02")));
    assert!(
        coarse.triangles.len() > done.triangles.len(),
        "detail keeps triangles"
    );
}

#[test]
fn finish_keeps_fine_brush_detail_better_than_a_coarse_build() {
    let detail = "brush w type=dam_standard size=0.3 strength=0.15\nstroke w from=-3,0,9.6 to=3,0,9.6\nstroke w from=-3,1,9.5 to=3,1,9.5";
    let fine = mesh_of(&build(&format!(
        "ball: blob 0,0,0 r=10\nremesh 0.1\n{detail}"
    )));
    let finished = mesh_of(&build(&format!(
        "ball: blob 0,0,0 r=10\nremesh 0.1\n{detail}\nfinish decimate=20000"
    )));
    let coarse = mesh_of(&build(&format!(
        "ball: blob 0,0,0 r=10\nremesh 0.6\n{detail}"
    )));
    assert!(coarse.triangles.len() <= finished.triangles.len() * 2);
    let (kept, _) = finished.error_from(&fine);
    let (lost, _) = coarse.error_from(&fine);
    assert!(kept < lost / 2.0, "finished {kept}, coarse {lost}");
}

#[test]
fn finish_focus_spends_the_budget_on_the_named_parts() {
    let two = "face: blob 0,0,0 r=5\nback: blob 12,0,0 r=5\nremesh 0.1\nbrush w type=dam_standard size=0.3 strength=0.15\nstroke w from=-2,-4.5,0 to=2,-4.5,0";
    let full = mesh_of(&build(two));
    let error_on = |m: &Mesh, part: &str| {
        let id = full.parts.iter().position(|p| p == part).unwrap() as u16;
        let picked: Vec<P3> = (0..full.positions.len())
            .filter(|&i| full.labels[i] == id)
            .map(|i| full.positions[i])
            .collect();
        let surface = gcad::mesh::grid::Surface::new(m);
        picked
            .iter()
            .map(|p| surface.closest(*p).unwrap().distance)
            .fold(0.0, f64::max)
    };
    let even = mesh_of(&build(&format!("{two}\nfinish decimate=3000")));
    let focused = mesh_of(&build(&format!(
        "{two}\nfinish decimate=3000 focus=face weight=20"
    )));
    assert_eq!(focused.triangles.len(), 3000);
    let on_face = |m: &Mesh| {
        let id = m.parts.iter().position(|p| p == "face").unwrap() as u16;
        m.triangles
            .iter()
            .filter(|t| m.labels[t[0] as usize] == id)
            .count()
    };
    assert!(
        on_face(&focused) * 10 > on_face(&even) * 14,
        "face triangles focused {} even {}",
        on_face(&focused),
        on_face(&even)
    );
    assert!(error_on(&focused, "face") < error_on(&even, "face"));
    let error = failure(&format!("{two}\nfinish decimate=3000 focus=nose"));
    assert!(error.contains("face, back"), "{error}");
}

#[test]
fn dynamesh_remeshes_a_sculpt_evenly_and_keeps_its_shape() {
    let source = format!("{BALL}\nselect sphere=0,0,10 r=2 falloff=2\nmove 0,0,6");
    let stretched = mesh_of(&build(&source));
    let model = build(&format!("{source}\nremesh dynamesh 0.25"));
    let even = mesh_of(&model);
    assert_eq!(even.open_edges(), 0);
    let rel = (even.volume() - stretched.volume()).abs() / stretched.volume();
    assert!(rel < 0.01, "{rel}");
    let long = |m: &Mesh| {
        m.triangles
            .iter()
            .flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3])))
            .map(|(a, b)| length(sub(m.positions[a as usize], m.positions[b as usize])))
            .fold(0.0, f64::max)
    };
    assert!(long(&stretched) > 1.0, "{}", long(&stretched));
    assert!(long(&even) < 0.25 * 2.0, "{}", long(&even));
    assert_eq!(even.parts, stretched.parts);
}

#[test]
fn hero_example_sculpts_at_full_detail_and_finishes_to_its_budget() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/mesh/hero.gcad"
    ))
    .expect("example");
    let model = build(&source);
    let mesh = mesh_of(&model);
    assert_eq!(mesh.triangles.len(), 14000);
    let rig = model.rigs();
    assert!(rig[0].1.skin_fits(&mesh));
    let lines = gcad::parse::parse_program(&source.replace("budget=14000", "budget=0")).unwrap();
    let result = gcad::model::run(&lines);
    let sculpted = result
        .steps
        .iter()
        .filter_map(|(line, out)| (line.op == "stroke").then(|| out.as_ref().unwrap().clone()))
        .collect::<Vec<_>>();
    assert!(sculpted.len() >= 12, "{}", sculpted.len());
    assert!(
        sculpted.iter().all(|s| !s.contains("nothing moved")),
        "{sculpted:?}"
    );
    let worst_start = sculpted
        .iter()
        .filter_map(|s| {
            s.split("started ")
                .nth(1)?
                .split(' ')
                .next()?
                .parse::<f64>()
                .ok()
        })
        .fold(0.0, f64::max);
    assert!(worst_start < 1.0, "{worst_start}");
    let full = result.model.meshes().last().unwrap().1.clone();
    assert!(full.triangles.len() > 400_000, "{}", full.triangles.len());
}

#[test]
fn brush_errors_name_the_choices() {
    let error = failure(&format!("{BALL}\nbrush b type=sponge size=1"));
    assert!(error.contains("dam_standard"), "{error}");
    let error = failure(&format!("{BALL}\ndab nope at=0,0,10"));
    assert!(error.contains("brush nope"), "{error}");
    let error = failure(&format!("{BALL}\nbrush b size=1"));
    assert!(error.contains("type="), "{error}");
}

#[test]
fn a_dab_off_the_surface_lands_on_it_and_says_how_far() {
    let text = summary(&format!(
        "{BALL}\nbrush b type=draw size=0.5 falloff=flat\nbrush b strength=0.1\ndab b at=0,0,30"
    ));
    assert!(text.contains("20.00 off the surface"), "{text}");
    let (before, after) =
        sculpt("brush b type=draw size=0.5 falloff=flat\nbrush b strength=0.1\ndab b at=0,0,30");
    near(
        moves(&before, &after)
            .iter()
            .map(|m| m.1)
            .fold(0.0, f64::max),
        0.1,
        1.0e-3,
    );
}

fn stamp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("gcad-tests").join(name);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn run_file(path: &std::path::Path) -> gcad::model::Run {
    gcad::model::run_path(path, &[], None).expect("runs")
}

#[test]
fn png_alpha_stamp_dents_like_a_procedural_one() {
    let dir = stamp_dir("png_stamp");
    let mut img = image::GrayImage::new(8, 8);
    for (x, y, p) in img.enumerate_pixels_mut() {
        p.0[0] = if (x + y) % 2 == 0 { 255 } else { 0 };
    }
    img.save(dir.join("dots.png")).expect("png");
    let file = dir.join("p.gcad");
    std::fs::write(
        &file,
        format!(
            "{BALL}\nbrush s type=stamp size=2 strength=0.1 alpha=dots.png(0.8)\ndab s at=0,0,10\n"
        ),
    )
    .expect("write");
    let run = run_file(&file);
    assert!(
        run.steps.iter().all(|(_, r)| r.is_ok()),
        "{:?}",
        run.steps.last()
    );
    let before = mesh_of(&build(BALL));
    let after = mesh_of(&run.model);
    let moved = moves(&before, &after);
    assert!(moved.iter().filter(|(_, m)| *m > 0.005).count() > 20);
    assert!(moved.iter().all(|(_, m)| *m <= 0.1 + 1.0e-9));
}

#[test]
fn missing_png_names_the_file() {
    let e = failure(&format!("{BALL}\nbrush s type=stamp size=2 alpha=nope.png"));
    assert!(e.contains("nope.png"), "{e}");
}

#[test]
fn px_size_needs_an_inspect_view_then_uses_it() {
    let dir = stamp_dir("px_size");
    let file = dir.join("q.gcad");
    let _ = std::fs::remove_file(gcad::inspect::sidecar(&file));
    std::fs::write(
        &file,
        format!("{BALL}\nbrush b type=draw size=40px strength=0.2\ndab b at=0,0,10\n"),
    )
    .expect("write");
    let run = run_file(&file);
    let e = format!("{:#}", run.steps.last().unwrap().1.as_ref().unwrap_err());
    assert!(e.contains("gcad inspect"), "{e}");
    std::fs::write(
        gcad::inspect::sidecar(&file),
        "view centre=0,0,0 span=100 pixels=10\n",
    )
    .expect("sidecar");
    let run = run_file(&file);
    assert!(
        run.steps.iter().all(|(_, r)| r.is_ok()),
        "{:?}",
        run.steps.last()
    );
    let after = mesh_of(&run.model);
    let before = mesh_of(&build(BALL));
    let reach = before
        .positions
        .iter()
        .zip(&after.positions)
        .filter(|(p, q)| length(sub(**q, **p)) > 1.0e-6)
        .map(|(p, _)| length(sub(*p, TOP)))
        .fold(0.0, f64::max);
    assert!(reach > 3.0 && reach <= 4.2, "reach {reach}");
}

fn set_age(path: &std::path::Path, seconds_ago: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_modified(when)
        .expect("mtime");
}

#[test]
fn px_size_is_pixels_times_units_per_pixel_from_the_sidecar() {
    let dir = stamp_dir("px_exact");
    let file = dir.join("e.gcad");
    std::fs::write(&file, format!("{BALL}\nbrush b type=draw size=40px\n")).expect("write");
    std::fs::write(
        gcad::inspect::sidecar(&file),
        "view centre=0,0,0 span=102.4 pixels=6.25\n",
    )
    .expect("sidecar");
    set_age(&file, 100);
    let run = run_file(&file);
    let said = run.steps.last().unwrap().1.as_ref().unwrap();
    assert!(said.contains("size 6.400"), "{said}");
    assert!(!said.contains("warning"), "{said}");
    let brush = run.model.scene.mesh_body.brushes.last().unwrap();
    assert!((brush.1.size - 6.4).abs() < 1.0e-12, "{}", brush.1.size);
}

#[test]
fn px_size_warns_when_the_file_changed_after_the_inspect() {
    let dir = stamp_dir("px_stale");
    let file = dir.join("s.gcad");
    std::fs::write(&file, format!("{BALL}\nbrush b type=draw size=40px\n")).expect("write");
    std::fs::write(
        gcad::inspect::sidecar(&file),
        "view centre=0,0,0 span=100 pixels=10\n",
    )
    .expect("sidecar");
    set_age(&gcad::inspect::sidecar(&file), 100);
    let run = run_file(&file);
    let said = run.steps.last().unwrap().1.as_ref().unwrap();
    assert!(said.contains("size 4.000"), "{said}");
    assert!(
        said.contains("changed since the last gcad inspect"),
        "{said}"
    );
}

#[test]
fn ramp_png_stamp_dent_profile() {
    let dir = stamp_dir("ramp");
    let mut img = image::GrayImage::new(64, 64);
    for (x, _, p) in img.enumerate_pixels_mut() {
        p.0[0] = (x * 255 / 63) as u8;
    }
    img.save(dir.join("ramp.png")).expect("png");
    let file = dir.join("r.gcad");
    std::fs::write(
        &file,
        format!("{BALL}\nbrush s type=stamp size=3 strength=0.5 falloff=smooth alpha=ramp.png(6)\ndab s at=0,0,10\n"),
    )
    .expect("write");
    let run = run_file(&file);
    assert!(
        run.steps.iter().all(|(_, r)| r.is_ok()),
        "{:?}",
        run.steps.last()
    );
    let before = mesh_of(&build(BALL));
    let after = mesh_of(&run.model);
    let mut rows: Vec<(f64, f64)> = Vec::new();
    for (p, q) in before.positions.iter().zip(&after.positions) {
        if length(sub(*p, TOP)) < 1.5 {
            rows.push((p[0] * 1.0e3 + p[1] * 1.0e-3, length(*q) - length(*p)));
        }
    }
    let by = |axis: usize| {
        let pts: Vec<(f64, f64)> = before
            .positions
            .iter()
            .zip(&after.positions)
            .filter(|(p, _)| length(sub(**p, TOP)) < 1.5)
            .map(|(p, q)| (p[axis], length(*q) - length(*p)))
            .collect();
        let n = pts.len() as f64;
        let (mx, my) = (
            pts.iter().map(|a| a.0).sum::<f64>() / n,
            pts.iter().map(|a| a.1).sum::<f64>() / n,
        );
        let cov: f64 = pts.iter().map(|a| (a.0 - mx) * (a.1 - my)).sum();
        let vx: f64 = pts.iter().map(|a| (a.0 - mx).powi(2)).sum();
        let vy: f64 = pts.iter().map(|a| (a.1 - my).powi(2)).sum();
        cov / (vx * vy).sqrt()
    };
    let (cx, cy) = (by(0), by(1));
    println!(
        "ramp correlation x {cx:.3} y {cy:.3} over {} vertices",
        rows.len()
    );
    let strongest = cx.abs().max(cy.abs());
    let weakest = cx.abs().min(cy.abs());
    assert!(strongest > 0.6, "x {cx} y {cy}");
    assert!(weakest < 0.3, "x {cx} y {cy}");
    let deepest = rows.iter().map(|r| r.1).fold(f64::INFINITY, f64::min);
    let highest = rows.iter().map(|r| r.1).fold(f64::NEG_INFINITY, f64::max);
    println!("ramp displacement {deepest:.3} .. {highest:.3}");
    assert!(deepest < -0.1 && highest > 0.1 && deepest >= -0.5 - 1.0e-9 && highest <= 0.5 + 1.0e-9);
}

fn glb_json(glb: &[u8]) -> serde_json::Value {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&glb[20..20 + len]).expect("json chunk")
}

fn glb_bin(glb: &[u8]) -> &[u8] {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    &glb[20 + len + 8..]
}

fn glb_of_source(source: &str) -> Vec<u8> {
    let model = build(source);
    gcad::export::glb_of(&model).expect("exports")
}

fn mesh_triangles(json: &serde_json::Value, mesh: usize) -> usize {
    json["meshes"][mesh]["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            json["accessors"][p["indices"].as_u64().unwrap() as usize]["count"]
                .as_u64()
                .unwrap() as usize
                / 3
        })
        .sum()
}

const WRINKLED: &str = "ball: blob 0,0,0 r=10\nremesh 0.15\nbrush w type=dam_standard size=0.5 strength=0.3\nstroke w from=-4,0,9.2 to=4,0,9.2\nlevel full";

#[test]
fn bake_normals_writes_a_tangent_space_map_that_tilts_on_the_wrinkle() {
    let glb = glb_of_source(&format!(
        "{WRINKLED}\nfinish decimate=3000\nbake normals size=256"
    ));
    let json = glb_json(&glb);
    assert_eq!(json["images"].as_array().unwrap().len(), 1);
    let primitive = &json["meshes"][0]["primitives"][0];
    assert!(primitive["attributes"]["TEXCOORD_0"].is_u64());
    assert!(primitive["attributes"]["TANGENT"].is_u64());
    let material = &json["materials"][primitive["material"].as_u64().unwrap() as usize];
    assert_eq!(material["normalTexture"]["index"], 0);
    let view = &json["bufferViews"][json["images"][0]["bufferView"].as_u64().unwrap() as usize];
    let at = view["byteOffset"].as_u64().unwrap() as usize;
    let png = &glb_bin(&glb)[at..at + view["byteLength"].as_u64().unwrap() as usize];
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 256);

    let high = mesh_of(&build(WRINKLED));
    let low = mesh_of(&build(&format!("{WRINKLED}\nfinish decimate=3000")));
    let baked = gcad::mesh::bake::bake_normals(&low, &high, 128);
    assert!(baked.covered > 128 * 128 / 5, "{}", baked.covered);
    let mut tilted = 0;
    let mut total = 0;
    for y in 0..128 {
        for x in 0..128 {
            if baked.pixels[y * 128 + x] == gcad::mesh::bake::FLAT {
                continue;
            }
            total += 1;
            if baked.tilt_at(x, y) > 10f64.to_radians() {
                tilted += 1;
            }
        }
    }
    assert!(
        tilted > 20,
        "pixels tilted past 10 degrees: {tilted} of {total}"
    );
    assert!(
        tilted * 5 < total,
        "most of the sphere stays near flat: {tilted} of {total}"
    );
}

#[test]
fn a_file_without_bake_has_no_uvs_or_images() {
    let json = glb_json(&glb_of_source(&format!("{WRINKLED}\nfinish decimate=3000")));
    assert!(json.get("images").is_none());
    assert!(json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"].is_null());
}

#[test]
fn lod_writes_three_meshes_with_the_asked_triangle_counts() {
    let glb = glb_of_source(&format!("{WRINKLED}\nfinish decimate=6000\nlod 3000 1000"));
    let json = glb_json(&glb);
    let meshes = json["meshes"].as_array().unwrap();
    assert_eq!(meshes.len(), 3);
    assert_eq!(meshes[1]["name"], "main_LOD1");
    assert_eq!(meshes[2]["name"], "main_LOD2");
    assert_eq!(mesh_triangles(&json, 0), 6000);
    for (mesh, wanted) in [(1, 3000), (2, 1000)] {
        let got = mesh_triangles(&json, mesh);
        assert!(
            got.abs_diff(wanted) <= 2,
            "LOD{mesh} has {got}, wanted {wanted}"
        );
    }
    let lod0 = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "main")
        .unwrap();
    assert_eq!(
        lod0["extensions"]["MSFT_lod"]["ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(json["extensionsUsed"][0], "MSFT_lod");
}

#[test]
fn lod_shares_count_from_the_full_detail_mesh() {
    let full = mesh_of(&build(WRINKLED)).triangles.len();
    let json = glb_json(&glb_of_source(&format!(
        "{WRINKLED}\nfinish decimate=4000\nlod 10%"
    )));
    let got = mesh_triangles(&json, 1);
    let wanted = full / 10;
    assert!(got.abs_diff(wanted) <= 2, "{got} vs {wanted}");
}

#[test]
fn lod_and_bake_refuse_nonsense() {
    assert!(failure(&format!("{BALL}\nlod")).contains("lower levels"));
    assert!(failure(&format!("{BALL}\nlod 150%")).contains("0% and 100%"));
    assert!(failure(&format!("{BALL}\nbake colours")).contains("normals"));
    assert!(failure(&format!("{BALL}\nbake normals size=5")).contains("16 to 8192"));
}

#[test]
fn finish_keep_creases_holds_the_edges_of_a_cube_sharper() {
    let source = "box: block 0,0,0 size=10,10,10 round=0.4\nremesh 0.25";
    let full = mesh_of(&build(source));
    let target = full.triangles.len() / 20;
    let plain = mesh_of(&build(&format!("{source}\nfinish decimate={target}")));
    let creased = mesh_of(&build(&format!(
        "{source}\nfinish decimate={target} keep=creases"
    )));
    let (plain_worst, plain_mean) = plain.error_from(&full);
    let (kept_worst, kept_mean) = creased.error_from(&full);
    assert!(
        kept_mean <= plain_mean * 1.05,
        "mean {kept_mean} vs {plain_mean}"
    );
    assert!(
        kept_worst <= plain_worst * 1.05,
        "worst {kept_worst} vs {plain_worst}"
    );
    assert!(failure(&format!("{BALL}\nfinish decimate=500 keep=bevels")).contains("uv"));
}

#[test]
fn skin_weights_and_uvs_survive_finish_in_the_same_primitive() {
    let source = format!(
        "{WRINKLED}\nfinish decimate=2000 keep=parts,creases,uv\nbone b head=0,0,-10 tail=0,0,10\nskin auto\nbake normals size=64\nlod 800"
    );
    let json = glb_json(&glb_of_source(&source));
    let a = &json["meshes"][0]["primitives"][0]["attributes"];
    assert!(a["JOINTS_0"].is_u64() && a["WEIGHTS_0"].is_u64() && a["TEXCOORD_0"].is_u64());
    let a1 = &json["meshes"][1]["primitives"][0]["attributes"];
    assert!(a1["JOINTS_0"].is_u64() && a1["WEIGHTS_0"].is_u64());
    assert_eq!(json["skins"].as_array().unwrap().len(), 1);
    let lod = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "main_LOD1")
        .unwrap();
    assert_eq!(lod["skin"], 0);
}
