use super::scene::{Ink, Rigged};
use crate::mesh::P3;
use crate::mesh::skin_check::{self, SkinReport};
use crate::mesh::skin_paint;
use egui::{Pos2, Rect, Stroke, Ui};
use egui_bench::prelude::*;
use egui_bench::viewer3d::{self, Projector, Viewer};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Show {
    #[default]
    Off,
    Bone,
    Influences,
    Problems,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Look,
    Add,
    Remove,
    Smooth,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Weights {
    pub show: Show,
    pub bone: usize,
    pub tool: Tool,
    pub radius: f64,
    pub strength: f64,
    pub mirror: bool,
    pub stroke: Vec<P3>,
    pub report: Option<(u64, Arc<SkinReport>)>,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            show: Show::Off,
            bone: 0,
            tool: Tool::Look,
            radius: 0.0,
            strength: 0.5,
            mirror: false,
            stroke: Vec::new(),
            report: None,
        }
    }
}

fn round(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

fn point(p: P3) -> String {
    format!("{},{},{}", round(p[0]), round(p[1]), round(p[2]))
}

pub fn spaced(points: &[P3], gap: f64) -> Vec<P3> {
    let mut out: Vec<P3> = Vec::new();
    for &p in points {
        if out
            .last()
            .is_none_or(|q| crate::mesh::length(crate::mesh::sub(p, *q)) >= gap)
        {
            out.push(p);
        }
    }
    if let (Some(&last), Some(&end)) = (out.last(), points.last())
        && last != end
    {
        out.push(end);
    }
    out
}

impl Weights {
    pub fn active(&self) -> bool {
        self.show != Show::Off
    }

    pub fn painting(&self) -> bool {
        self.active() && self.tool != Tool::Look
    }

    pub fn fit(&mut self, rigged: &Rigged) {
        if self.bone >= rigged.rig.bones.len() {
            self.bone = 0;
        }
        if self.radius <= 0.0 {
            let (lo, hi) = rigged.rest.bounds().unwrap_or_default();
            let size = crate::mesh::length(crate::mesh::sub(hi, lo));
            self.radius = (size / 40.0 * 1000.0).round() / 1000.0;
        }
    }

    pub fn line(&self, rigged: &Rigged) -> Option<String> {
        let points = spaced(&self.stroke, self.radius / 2.0);
        let (first, rest) = points.split_first()?;
        let rest: String = rest.iter().map(|p| format!(" {}", point(*p))).collect();
        let mirror = if self.mirror { " mirror=x" } else { "" };
        let bone = &rigged.rig.bones.get(self.bone)?.name;
        let r = round(self.radius);
        Some(match self.tool {
            Tool::Look => return None,
            Tool::Add | Tool::Remove => {
                let sign = if self.tool == Tool::Remove { "-" } else { "" };
                format!(
                    "skin paint bone={bone} sphere={}{rest} r={r} falloff={r} strength={sign}{}{mirror}",
                    point(*first),
                    round(self.strength)
                )
            }
            Tool::Smooth => format!(
                "skin smooth sphere={}{rest} r={r} passes={}{mirror}",
                point(*first),
                (self.strength * 20.0).round().max(1.0)
            ),
        })
    }

    pub fn report(&mut self, rigged: &Rigged, key: u64) -> Arc<SkinReport> {
        if let Some((k, r)) = &self.report
            && *k == key
        {
            return r.clone();
        }
        let report = Arc::new(skin_check::check(&rigged.rest, &rigged.rig).unwrap_or_default());
        self.report = Some((key, report.clone()));
        report
    }

    pub fn colours(&mut self, rigged: &Rigged, key: u64) -> Option<Vec<[f64; 3]>> {
        let skin = rigged.rig.skin.as_ref()?;
        Some(match self.show {
            Show::Off => return None,
            Show::Bone => skin_paint::bone_colours(skin, self.bone),
            Show::Influences => skin_paint::influence_colours(skin),
            Show::Problems => {
                let report = self.report(rigged, key);
                skin_paint::problem_colours(&rigged.rest, &rigged.rig, &report)
            }
        })
    }
}

pub fn recolour(
    mesh: &viewer3d::Scene<Ink>,
    rigged: &[Rigged],
    colours: &[Vec<[f64; 3]>],
) -> Arc<viewer3d::Scene<Ink>> {
    let mut surfaces = mesh.surfaces.clone();
    for (r, paint) in rigged.iter().zip(colours) {
        for (index, vertices) in &r.slots {
            let Some(surface) = surfaces.get_mut(*index) else {
                continue;
            };
            surface.colours = vertices
                .iter()
                .map(|&i| {
                    paint
                        .get(i as usize)
                        .copied()
                        .unwrap_or([0.5; 3])
                        .map(|v| v as f32)
                })
                .collect();
            surface.texture = None;
            surface.material = Ink::Body;
        }
    }
    Arc::new(viewer3d::Scene {
        surfaces,
        lines: mesh.lines.clone(),
        images: mesh.images.clone(),
        centre: mesh.centre,
        radius: mesh.radius,
        ..Default::default()
    })
}

pub fn brush_ring(
    p: &egui::Painter,
    projector: &Projector,
    at: P3,
    radius: f64,
    colour: egui::Color32,
) {
    let Some(centre) = projector.project(at.map(|v| v as f32)) else {
        return;
    };
    let side = [
        at[0] as f32 + projector.right[0] * radius as f32,
        at[1] as f32 + projector.right[1] * radius as f32,
        at[2] as f32 + projector.right[2] * radius as f32,
    ];
    let Some(edge) = projector.project(side) else {
        return;
    };
    p.circle_stroke(centre, centre.distance(edge), Stroke::new(1.5, colour));
}

pub fn hit(viewer: &Viewer<Ink>, rect: Rect, at: Pos2) -> Option<P3> {
    viewer.pick(rect, at).map(|h| h.point.map(|v| v as f64))
}

pub fn card(ui: &mut Ui, weights: &mut Weights, rigged: &Rigged, key: u64) {
    let rig = &rigged.rig;
    if rig.skin.is_none() {
        return;
    }
    let mut show = weights.show;
    let mut flip = false;
    let lit = weights.painting().then_some(READOUT);
    let on = show != Show::Off;
    egui_bench::panel::card(
        ui,
        lit,
        |ui| {
            Line::new().legend("weights").show(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if toggle(ui, "show", on).clicked() {
                    flip = true;
                }
            });
        },
        |ui| {
            if !on {
                hint(
                    ui,
                    "colour the mesh by skin weight, and paint weights onto it",
                );
                return;
            }
            ui.horizontal_wrapped(|ui| {
                for (s, label) in [
                    (Show::Bone, "bone"),
                    (Show::Influences, "influences"),
                    (Show::Problems, "problems"),
                ] {
                    if toggle(ui, label, show == s).clicked() {
                        show = s;
                    }
                }
            });
            ui.allocate_ui(egui::vec2(ui.available_width(), KEY_H), |ui| {
                choice(
                    ui,
                    "weights bone",
                    &mut weights.bone,
                    rig.bones
                        .iter()
                        .enumerate()
                        .map(|(i, b)| (i, b.name.clone())),
                );
            });
            match show {
                Show::Influences => hint(ui, "grey 0, blue 1, green 2, yellow 3, red 4 bones"),
                Show::Problems => {
                    let r = weights.report(rigged, key);
                    Line::new()
                        .legend("steep edges")
                        .measured(r.steep_edges.to_string())
                        .size(11.0)
                        .show(ui);
                    Line::new()
                        .legend("loose cores")
                        .measured(r.loose.len().to_string())
                        .size(11.0)
                        .show(ui);
                    Line::new()
                        .legend("flips")
                        .measured(r.quality.flipped.to_string())
                        .size(11.0)
                        .show(ui);
                }
                _ => hint(ui, "blue 0 to red 1 on the chosen bone"),
            }
            ui.add_space(4.0);
            Line::new().legend("brush").show(ui);
            ui.horizontal_wrapped(|ui| {
                for (t, label) in [
                    (Tool::Look, "look"),
                    (Tool::Add, "add"),
                    (Tool::Remove, "remove"),
                    (Tool::Smooth, "smooth"),
                ] {
                    if toggle(ui, label, weights.tool == t).clicked() {
                        weights.tool = t;
                    }
                }
                if toggle(ui, "mirror x", weights.mirror).clicked() {
                    weights.mirror = !weights.mirror;
                }
            });
            let (lo, hi) = rigged.rest.bounds().unwrap_or_default();
            let size = crate::mesh::length(crate::mesh::sub(hi, lo)).max(1.0e-3);
            ui.add(
                egui::Slider::new(&mut weights.radius, size / 200.0..=size / 4.0)
                    .text("radius")
                    .logarithmic(true),
            );
            ui.add(egui::Slider::new(&mut weights.strength, 0.05..=1.0).text("strength"));
            if weights.tool != Tool::Look {
                hint(
                    ui,
                    "drag on the mesh to paint; each stroke adds one skin line to the file. shift-drag still pans",
                );
            }
        },
    );
    if flip {
        show = if on { Show::Off } else { Show::Bone };
    }
    weights.show = show;
    ui.add_space(8.0);
}

pub fn append_line(source: &str, line: &str) -> String {
    let mut out = source.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATURE: &str = include_str!("../../examples/mesh/rigged_creature.gcad");

    fn rigged() -> Rigged {
        let lines = crate::parse::parse_program(CREATURE).expect("parses");
        let model = crate::model::run(&lines).model;
        let (name, rest) = model.rest_meshes().pop().expect("a mesh");
        Rigged {
            rest,
            rig: model.rig_of(&name).expect("a rig").clone(),
            slots: Vec::new(),
        }
    }

    #[test]
    fn a_stroke_becomes_one_skin_line_the_file_accepts() {
        let r = rigged();
        let mut w = Weights {
            show: Show::Bone,
            tool: Tool::Add,
            bone: r.rig.bone_index("hind_l").unwrap(),
            radius: 0.8,
            strength: 0.4,
            mirror: true,
            stroke: vec![[3.0, 8.0, 8.0], [3.1, 8.0, 8.0], [3.0, 7.0, 8.5]],
            ..Default::default()
        };
        let line = w.line(&r).unwrap();
        assert_eq!(
            line,
            "skin paint bone=hind_l sphere=3,8,8 3,7,8.5 r=0.8 falloff=0.8 strength=0.4 mirror=x"
        );
        w.tool = Tool::Remove;
        assert!(w.line(&r).unwrap().contains("strength=-0.4"));
        w.tool = Tool::Smooth;
        assert_eq!(
            w.line(&r).unwrap(),
            "skin smooth sphere=3,8,8 3,7,8.5 r=0.8 passes=8 mirror=x"
        );
        let source = append_line(CREATURE, &line);
        let lines = crate::parse::parse_program(&source).expect("parses");
        let run = crate::model::run(&lines);
        assert!(run.steps.iter().all(|(_, s)| s.is_ok()));
        w.tool = Tool::Look;
        assert!(w.line(&r).is_none());
    }

    #[test]
    fn the_bone_view_paints_the_chosen_bone_hot() {
        let r = rigged();
        let bone = r.rig.bone_index("hind_l").unwrap();
        let mut w = Weights {
            show: Show::Bone,
            bone,
            ..Default::default()
        };
        let colours = w.colours(&r, 1).unwrap();
        let skin = r.rig.skin.as_ref().unwrap();
        let rigid = skin.iter().position(|v| v == &vec![(bone, 1.0)]).unwrap();
        let none = skin
            .iter()
            .position(|v| v.iter().all(|(b, _)| *b != bone))
            .unwrap();
        assert_eq!(colours[rigid], skin_paint::heat_colour(1.0));
        assert_eq!(colours[none], skin_paint::heat_colour(0.0));
        w.show = Show::Problems;
        let problems = w.colours(&r, 1).unwrap();
        assert_eq!(problems.len(), r.rest.positions.len());
        w.show = Show::Off;
        assert!(w.colours(&r, 1).is_none());
    }

    #[test]
    fn close_points_merge_and_the_end_is_kept() {
        let pts = [
            [0.0, 0.0, 0.0],
            [0.1, 0.0, 0.0],
            [0.6, 0.0, 0.0],
            [0.7, 0.0, 0.0],
        ];
        assert_eq!(
            spaced(&pts, 0.5),
            vec![[0.0, 0.0, 0.0], [0.6, 0.0, 0.0], [0.7, 0.0, 0.0]]
        );
    }
}
