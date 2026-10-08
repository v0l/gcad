use super::gl::{self, BACKGROUND, Camera, Cut, Placement, Projector};
use super::scene::{self, Highlight, Scene, V3};
use crate::geometry;
use crate::model::{
    Joint, JointKind, Rig, Snapshot, explode_offsets, label_of, posed, snapshots_path,
};
use crate::parse::Line as SourceLine;
use crate::select;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_bench::prelude::*;
use monstertruck::modeling::{Matrix4, Solid, SquareMatrix, builder};
use notify::{RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Program {
    lines: Vec<SourceLine>,
    snapshots: Vec<Snapshot>,
    error: Option<String>,
}

fn evaluate(path: &std::path::Path, vars: &[(String, f64)]) -> Program {
    match snapshots_path(path, vars) {
        Ok((lines, snapshots)) => Program {
            snapshots,
            lines,
            error: None,
        },
        Err(error) => Program {
            error: Some(format!("{error:#}")),
            ..Default::default()
        },
    }
}

#[derive(Clone, PartialEq)]
struct SceneKey {
    generation: u64,
    line: usize,
    query: String,
}

struct Shown {
    key: SceneKey,
    scene: Arc<Scene>,
    faces: usize,
    bounds: ([f64; 3], [f64; 3]),
    query: Option<Result<(usize, usize), String>>,
    joints: Vec<Joint>,
    rig: Rig,
    explode: Vec<(String, monstertruck::modeling::Vector3)>,
    solids: Vec<(String, Solid)>,
    assembly: bool,
}

enum Status {
    Ok,
    Failed,
    NotRun,
}

#[derive(Clone, Copy)]
struct Pick {
    point: V3,
    face: Option<usize>,
    vertex: bool,
}

struct Browser {
    dir: PathBuf,
    typed: String,
}

pub struct App {
    path: Option<PathBuf>,
    vars: Vec<(String, f64)>,
    program: Arc<Program>,
    generation: u64,
    selected: usize,
    follow: bool,
    query: String,
    shown: Option<Shown>,
    building: Option<(SceneKey, Receiver<Shown>)>,
    cam: Camera,
    events: Option<Receiver<()>>,
    _watcher: Option<notify::RecommendedWatcher>,
    dirty: Option<Instant>,
    loading: Option<(Receiver<Program>, Instant)>,
    loaded: Instant,
    took: Duration,
    measuring: bool,
    picks: Vec<Pick>,
    joint_values: Vec<f64>,
    explode: f64,
    section: Option<(usize, f32, bool)>,
    joint_key: Vec<String>,
    hidden: Vec<bool>,
    clashes: Option<Result<Vec<String>, ()>>,
    clash_job: Option<Receiver<Vec<String>>>,
    browser: Option<Browser>,
}

fn changes_content(kind: &notify::EventKind) -> bool {
    use notify::EventKind;
    use notify::event::{AccessKind, AccessMode, ModifyKind};
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) => true,
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        _ => false,
    }
}

fn flat(m: Matrix4) -> Placement {
    let c = [m.x, m.y, m.z, m.w];
    let mut out = [0.0f32; 16];
    for (i, column) in c.iter().enumerate() {
        out[i * 4] = column.x as f32;
        out[i * 4 + 1] = column.y as f32;
        out[i * 4 + 2] = column.z as f32;
        out[i * 4 + 3] = column.w as f32;
    }
    out
}

fn apply(m: &Placement, p: V3) -> V3 {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn hit_triangle(origin: V3, direction: V3, [a, b, c]: [V3; 3]) -> Option<f32> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(direction, e2);
    let det = dot(e1, p);
    if det.abs() < 1.0e-12 {
        return None;
    }
    let s = sub(origin, a);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    let t = dot(e2, q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
}

const VIEWS: [(&str, f32, f32); 7] = [
    ("iso", std::f32::consts::FRAC_PI_4, 0.6155),
    ("top", 0.0, 1.5699),
    ("front", 0.0, 0.0),
    ("right", std::f32::consts::FRAC_PI_2, 0.0),
    ("back", std::f32::consts::PI, 0.0),
    ("left", -std::f32::consts::FRAC_PI_2, 0.0),
    ("bottom", 0.0, -1.5699),
];

const CUBE_FACES: [(V3, &str); 6] = [
    ([0.0, 0.0, 1.0], "TOP"),
    ([0.0, 0.0, -1.0], "BOTTOM"),
    ([0.0, -1.0, 0.0], "FRONT"),
    ([0.0, 1.0, 0.0], "BACK"),
    ([1.0, 0.0, 0.0], "RIGHT"),
    ([-1.0, 0.0, 0.0], "LEFT"),
];

fn inside(polygon: &[Pos2], p: Pos2) -> bool {
    (0..polygon.len()).fold(false, |inside, i| {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            !inside
        } else {
            inside
        }
    })
}

impl App {
    fn new(
        cc: &eframe::CreationContext,
        path: Option<PathBuf>,
        query: String,
        line: Option<usize>,
        vars: Vec<(String, f64)>,
    ) -> Self {
        egui_bench::install(&cc.egui_ctx);
        let mut app = App {
            path: None,
            vars,
            program: Arc::new(Program::default()),
            generation: 0,
            selected: line.map_or(0, |n| n.saturating_sub(1)),
            follow: line.is_none(),
            query,
            shown: None,
            building: None,
            cam: Camera::default(),
            events: None,
            _watcher: None,
            dirty: None,
            loading: None,
            loaded: Instant::now(),
            took: Duration::ZERO,
            measuring: false,
            picks: Vec::new(),
            joint_values: Vec::new(),
            explode: 0.0,
            section: None,
            joint_key: Vec::new(),
            hidden: Vec::new(),
            clashes: None,
            clash_job: None,
            browser: None,
        };
        match path {
            Some(path) => app.open(path, &cc.egui_ctx),
            None => app.browser = Some(Browser::at(std::env::current_dir().unwrap_or_default())),
        }
        app
    }

    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "linecad - {}",
            path.display()
        )));
        self.path = Some(path);
        self.program = Arc::new(Program::default());
        self.shown = None;
        self.building = None;
        self.picks.clear();
        self.hidden.clear();
        self.clashes = None;
        self.follow = true;
        self.cam = Camera::default();
        self._watcher = None;
        self.events = None;
        self.reload(ctx);
        self.watch(ctx.clone());
    }

    fn watch(&mut self, ctx: egui::Context) {
        let Some(file) = self.path.clone() else {
            return;
        };
        let (tx, rx) = channel();
        let watched = file.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && changes_content(&event.kind)
                && event.paths.iter().any(|p| {
                    p == &watched
                        || p.parent() == watched.parent()
                            && p.extension().is_some_and(|e| e == "lcad" || e == "lasm")
                })
            {
                let _ = tx.send(());
                ctx.request_repaint();
            }
        });
        if let Ok(mut watcher) = watcher
            && let Some(dir) = file.parent()
            && watcher.watch(dir, RecursiveMode::NonRecursive).is_ok()
        {
            self._watcher = Some(watcher);
            self.events = Some(rx);
        }
    }

    fn reload(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let (tx, rx) = channel();
        let (ctx, vars) = (ctx.clone(), self.vars.clone());
        std::thread::spawn(move || {
            let _ = tx.send(evaluate(&path, &vars));
            ctx.request_repaint();
        });
        self.loading = Some((rx, Instant::now()));
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.events {
            while rx.try_recv().is_ok() {
                self.dirty = Some(Instant::now());
            }
        }
        if let Some(rx) = &self.clash_job {
            match rx.try_recv() {
                Ok(found) => {
                    self.clashes = Some(Ok(found));
                    self.clash_job = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => {
                    self.clashes = Some(Err(()));
                    self.clash_job = None;
                }
            }
        }
        if let Some((key, rx)) = &self.building {
            match rx.try_recv() {
                Ok(shown) => {
                    if shown.key == *key {
                        self.adopt(shown);
                    }
                    self.building = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
                Err(TryRecvError::Disconnected) => self.building = None,
            }
        }
        if let Some((rx, started)) = &self.loading {
            match rx.try_recv() {
                Ok(program) => {
                    self.took = started.elapsed();
                    self.loading = None;
                    self.install(program);
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    return;
                }
                Err(TryRecvError::Disconnected) => self.loading = None,
            }
        }
        if let Some(t) = self.dirty {
            let wait = Duration::from_millis(150);
            if t.elapsed() >= wait {
                self.dirty = None;
                self.reload(ctx);
            } else {
                ctx.request_repaint_after(wait - t.elapsed());
            }
        }
    }

    fn adopt(&mut self, shown: Shown) {
        let names: Vec<String> = shown.joints.iter().map(|j| j.name.clone()).collect();
        if names != self.joint_key {
            self.joint_values = shown.joints.iter().map(|j| j.value).collect();
            self.joint_key = names;
        }
        if self.hidden.len() != shown.scene.parts.len() {
            self.hidden = vec![false; shown.scene.parts.len()];
        }
        self.shown = Some(shown);
    }

    fn install(&mut self, program: Program) {
        self.generation += 1;
        self.loaded = Instant::now();
        let last = program.snapshots.len().saturating_sub(1);
        if self.follow || self.selected >= program.lines.len() {
            self.selected = last;
        }
        self.program = Arc::new(program);
        self.clashes = None;
    }

    fn status(&self, index: usize) -> Status {
        match self.program.snapshots.get(index).map(|s| s.result.is_ok()) {
            Some(true) => Status::Ok,
            Some(false) => Status::Failed,
            None => Status::NotRun,
        }
    }

    fn refresh_scene(&mut self, ctx: &egui::Context) {
        let key = SceneKey {
            generation: self.generation,
            line: self.selected,
            query: self.query.trim().to_string(),
        };
        if self.shown.as_ref().is_some_and(|s| s.key == key)
            || self.building.as_ref().is_some_and(|(k, _)| *k == key)
        {
            return;
        }
        let program = self.program.clone();
        let index = self.selected.min(program.snapshots.len().saturating_sub(1));
        if program.snapshots.is_empty() {
            self.shown = None;
            return;
        }
        let (tx, rx) = channel();
        let job_key = key.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let snapshot = &program.snapshots[index];
            if let Some(shown) = build_shown(snapshot, job_key) {
                let _ = tx.send(shown);
            }
            ctx.request_repaint();
        });
        self.building = Some((key, rx));
    }

    fn placements(&self) -> Vec<(Placement, bool)> {
        let Some(shown) = &self.shown else {
            return Vec::new();
        };
        let values = if self.joint_values.len() == shown.joints.len() {
            self.joint_values.clone()
        } else {
            shown.joints.iter().map(|j| j.value).collect()
        };
        let moved = posed(&shown.joints, &values);
        let offsets = explode_offsets(&shown.joints, &shown.explode, &moved);
        shown
            .scene
            .parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                let m = moved
                    .get(&part.name)
                    .copied()
                    .unwrap_or_else(Matrix4::identity);
                let m = exploded(m, offsets.get(&part.name), self.explode);
                (flat(m), !self.hidden.get(i).copied().unwrap_or(false))
            })
            .collect()
    }

    fn header(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            Line::new()
                .legend("linecad")
                .value(
                    self.path
                        .as_ref()
                        .map_or_else(|| "no file".to_string(), |p| p.display().to_string()),
                )
                .show(ui);
            if toggle(ui, "open", self.browser.is_some()).clicked() {
                self.browser = match self.browser {
                    Some(_) => None,
                    None => Some(Browser::at(
                        self.path
                            .as_ref()
                            .and_then(|p| p.parent())
                            .map(std::path::Path::to_path_buf)
                            .or_else(|| std::env::current_dir().ok())
                            .unwrap_or_default(),
                    )),
                };
            }
            if toggle(ui, "measure", self.measuring).clicked() {
                self.measuring = !self.measuring;
                self.picks.clear();
            }
            if toggle(ui, "section", self.section.is_some()).clicked() {
                self.section = match self.section {
                    Some(_) => None,
                    None => Some((1, 0.5, false)),
                };
            }
            if toggle(ui, "ortho", self.cam.ortho).clicked() {
                self.cam.ortho = !self.cam.ortho;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.path.is_none() {
                    return;
                }
                let failed = self.program.error.is_some()
                    || self.program.snapshots.iter().any(|s| s.result.is_err());
                lamp(ui, if failed { "error" } else { "ok" }, !failed, failed);
                let busy = self.loading.is_some();
                lamp(
                    ui,
                    if busy { "building" } else { "live" },
                    self.events.is_some() && !busy,
                    false,
                )
                .on_hover_text(format!(
                    "rebuilds when the file is saved; last build took {:.2}s, {:.0}s ago",
                    self.took.as_secs_f32(),
                    self.loaded.elapsed().as_secs_f32()
                ));
                lamp(
                    ui,
                    &format!("{} lines", self.program.lines.len()),
                    false,
                    false,
                );
                if self.path.is_some() && toggle(ui, "rebuild", false).clicked() {
                    self.reload(ctx);
                }
            });
        });
        ui.add_space(6.0);
    }

    fn lines(&mut self, ui: &mut Ui) {
        if let Some(error) = &self.program.error {
            card(
                ui,
                Some(FAULT),
                |ui| {
                    Line::new().legend("file did not parse").show(ui);
                },
                |ui| {
                    note(ui, error, FAULT);
                },
            );
            ui.add_space(6.0);
        }
        let program = self.program.clone();
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 20.0, program.lines.len(), |ui, range| {
                for index in range {
                    let line = &program.lines[index];
                    let (rect, resp) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
                    let on = index == self.selected;
                    let p = ui.painter();
                    if on {
                        p.rect_filled(rect, 0.0, PANEL);
                        p.rect_filled(
                            Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
                            0.0,
                            READOUT,
                        );
                    } else if resp.hovered() {
                        p.rect_filled(rect, 0.0, BAND);
                    }
                    let (dot, text_colour) = match self.status(index) {
                        Status::Ok => (OK, VALUE),
                        Status::Failed => (FAULT, FAULT),
                        Status::NotRun => (ETCH, LEGEND),
                    };
                    p.circle_filled(Pos2::new(rect.left() + 11.0, rect.center().y), 3.0, dot);
                    p.text(
                        Pos2::new(rect.left() + 40.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        line.number.to_string(),
                        figure(11.0),
                        LEGEND,
                    );
                    p.with_clip_rect(rect).text(
                        Pos2::new(rect.left() + 48.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &line.text,
                        mono(12.0),
                        if on {
                            text_colour
                        } else {
                            text_colour.gamma_multiply(0.85)
                        },
                    );
                    if resp.clicked() {
                        clicked = Some(index);
                    }
                }
            });
        if let Some(index) = clicked {
            self.select(index);
        }
    }

    fn select(&mut self, index: usize) {
        self.selected = index;
        self.follow = index + 1 >= self.program.snapshots.len();
    }

    fn line_card(&mut self, ui: &mut Ui) {
        let program = self.program.clone();
        let Some(line) = program.lines.get(self.selected) else {
            return;
        };
        let snapshot = program.snapshots.get(self.selected);
        let rail = match snapshot.map(|s| s.result.is_ok()) {
            Some(true) => OK,
            Some(false) => FAULT,
            None => ETCH,
        };
        card(
            ui,
            Some(rail),
            |ui| {
                Line::new()
                    .legend("line")
                    .value(line.number.to_string())
                    .legend(&line.op)
                    .show(ui);
            },
            |ui| {
                Line::new().value(&line.text).size(12.0).wrapped(ui);
                if line.label.is_some()
                    || matches!(
                        line.op.as_str(),
                        "extrude" | "cut" | "hole" | "revolve" | "sweep" | "fillet" | "chamfer"
                    )
                {
                    Line::new()
                        .legend("faces")
                        .set(label_of(line))
                        .note("highlighted")
                        .show(ui);
                }
                match snapshot.map(|s| &s.result) {
                    Some(Ok(summary)) => note(ui, summary, LEGEND),
                    Some(Err(error)) => note(ui, error, FAULT),
                    None => note(ui, "not run, an earlier line failed", LEGEND),
                }
            },
        );
        ui.add_space(8.0);
    }

    fn measure_card(&mut self, ui: &mut Ui) {
        let Some(shown) = &self.shown else { return };
        let scene = shown.scene.clone();
        let mut clear = false;
        card(
            ui,
            Some(TRACE),
            |ui| {
                Line::new().legend("measure").show(ui);
            },
            |ui| {
                if self.picks.is_empty() {
                    hint(
                        ui,
                        "click the model to pick a point; a corner snaps. Pick two to measure between them",
                    );
                }
                for (n, pick) in self.picks.iter().enumerate() {
                    let p = pick.point;
                    Line::new()
                        .legend(if n == 0 { "a" } else { "b" })
                        .value(format!("{:.3}, {:.3}, {:.3}", p[0], p[1], p[2]))
                        .note(if pick.vertex { "corner" } else { "on face" })
                        .size(11.0)
                        .show(ui);
                    if let Some(info) = pick.face.and_then(|f| scene.faces.get(f)) {
                        let part = scene.parts.get(info.body).map_or("", |p| p.name.as_str());
                        note(
                            ui,
                            format!("{part}: {}, area {:.3}", info.kind, info.area),
                            LEGEND,
                        );
                    }
                }
                if let [a, b] = self.picks[..] {
                    let d = sub(b.point, a.point);
                    let length = dot(d, d).sqrt();
                    readouts(
                        ui,
                        &[
                            ("distance", format!("{length:.3}"), TRACE),
                            ("dx", format!("{:.3}", d[0].abs()), VALUE),
                            ("dy", format!("{:.3}", d[1].abs()), VALUE),
                            ("dz", format!("{:.3}", d[2].abs()), VALUE),
                        ],
                    );
                }
                if !self.picks.is_empty() && toggle(ui, "clear", false).clicked() {
                    clear = true;
                }
            },
        );
        if clear {
            self.picks.clear();
        }
        ui.add_space(8.0);
    }

    fn parts_card(&mut self, ui: &mut Ui) {
        let Some(shown) = &self.shown else { return };
        let scene = shown.scene.clone();
        let joints = shown.joints.clone();
        let rig = shown.rig.clone();
        let exploded = !shown.explode.is_empty();
        let solids = shown.solids.clone();
        let (min, max) = shown.bounds;
        let faces = shown.faces;
        card(
            ui,
            None,
            |ui| {
                Line::new().legend("parts").show(ui);
            },
            |ui| {
                for (i, part) in scene.parts.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                        let [r, g, b] = part.colour.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8);
                        ui.painter()
                            .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
                        let visible = !self.hidden.get(i).copied().unwrap_or(false);
                        if toggle(ui, &part.name, visible).clicked()
                            && let Some(flag) = self.hidden.get_mut(i)
                        {
                            *flag = !*flag;
                        }
                        Line::new()
                            .legend("volume")
                            .measured(format!("{:.1}", part.volume))
                            .show(ui);
                    });
                }
                Line::new()
                    .legend("faces")
                    .measured(faces.to_string())
                    .legend("size")
                    .measured(format!(
                        "{:.1} x {:.1} x {:.1}",
                        max[0] - min[0],
                        max[1] - min[1],
                        max[2] - min[2]
                    ))
                    .size(11.0)
                    .show(ui);
                if exploded {
                    ui.horizontal(|ui| {
                        Line::new().legend("explode").size(11.0).show(ui);
                        ui.add(egui::Slider::new(&mut self.explode, 0.0..=1.0).show_value(false));
                    });
                }
            },
        );
        ui.add_space(8.0);
        if !joints.iter().any(Joint::movable) {
            return;
        }
        let mut check = false;
        card(
            ui,
            None,
            |ui| {
                Line::new().legend("joints").show(ui);
            },
            |ui| {
                for (i, joint) in joints.iter().enumerate() {
                    if !joint.movable() || i >= self.joint_values.len() {
                        continue;
                    }
                    let (low, high) = joint.range;
                    let step = match joint.kind {
                        JointKind::Turn { .. } => 0.5,
                        _ => 0.05,
                    };
                    let driver = rig.driver_of(&joint.name).map(|c| c.driver.clone());
                    let nudged = |delta: f64| {
                        let to = (self.joint_values[i] + delta).clamp(low, high);
                        (to != self.joint_values[i])
                            .then(|| rig.drive(&self.joint_values, i, to).err())
                    };
                    let stuck = match (nudged(step), nudged(-step)) {
                        (Some(Some(why)), Some(Some(_)))
                        | (Some(Some(why)), None)
                        | (None, Some(Some(why))) => Some(why),
                        _ => None,
                    };
                    Line::new()
                        .legend(&joint.name)
                        .note(format!("{} on {}", joint.child, joint.parent))
                        .size(11.0)
                        .show(ui);
                    let mut value = self.joint_values[i];
                    ui.add_enabled(
                        stuck.is_none() && driver.is_none(),
                        egui::Slider::new(&mut value, low..=high)
                            .suffix(joint.unit())
                            .clamping(egui::SliderClamping::Always),
                    );
                    if value != self.joint_values[i]
                        && let Ok((settled, _)) = rig.drive(&self.joint_values, i, value)
                    {
                        self.joint_values = settled;
                    }
                    if let Some(driver) = driver {
                        note(ui, format!("driven by {driver}"), LEGEND);
                    } else if let Some(why) = stuck {
                        note(ui, format!("locked: {why}"), WARN);
                    }
                }
                ui.horizontal(|ui| {
                    if toggle(ui, "as file", false).clicked() {
                        self.joint_values = joints.iter().map(|j| j.value).collect();
                    }
                    if toggle(ui, "check clashes", self.clash_job.is_some()).clicked()
                        && self.clash_job.is_none()
                    {
                        check = true;
                    }
                });
                match &self.clashes {
                    _ if self.clash_job.is_some() => note(ui, "checking", LEGEND),
                    Some(Ok(found)) if found.is_empty() => note(ui, "no parts overlap here", OK),
                    Some(Ok(found)) => found.iter().for_each(|f| note(ui, f, FAULT)),
                    Some(Err(())) => note(ui, "the check failed", FAULT),
                    None => {}
                }
            },
        );
        if check {
            let values = self.joint_values.clone();
            let (tx, rx) = channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let moved = posed(&joints, &values);
                let placed: Vec<(String, Solid)> = solids
                    .iter()
                    .map(|(name, solid)| {
                        let m = moved.get(name).copied().unwrap_or_else(Matrix4::identity);
                        (name.clone(), builder::transformed(solid, m))
                    })
                    .collect();
                let meshes: Vec<geometry::Meshed> = placed
                    .iter()
                    .map(|(_, s)| geometry::Meshed::new(s))
                    .collect();
                let mut found = Vec::new();
                for i in 0..placed.len() {
                    for j in i + 1..placed.len() {
                        let shared = geometry::overlap_of(&meshes[i], &meshes[j], 96);
                        if shared > 1.0e-6 * meshes[i].volume().abs().max(1.0) {
                            found.push(format!(
                                "{} and {} share about {shared:.3}",
                                placed[i].0, placed[j].0
                            ));
                        }
                    }
                }
                let _ = tx.send(found);
                ctx.request_repaint();
            });
            self.clash_job = Some(rx);
            self.clashes = None;
        }
        ui.add_space(8.0);
    }

    fn select_card(&mut self, ui: &mut Ui) {
        card(
            ui,
            None,
            |ui| {
                Line::new().legend("select").show(ui);
            },
            |ui| {
                field(ui, &mut self.query, "base.end&base.side");
                match self.shown.as_ref().and_then(|s| s.query.clone()) {
                    Some(Ok((faces, edges))) => {
                        Line::new()
                            .legend("faces")
                            .measured(faces.to_string())
                            .legend("edges")
                            .measured(edges.to_string())
                            .show(ui);
                    }
                    Some(Err(error)) => note(ui, error, FAULT),
                    None => hint(
                        ui,
                        "faces: label, label.group, >Z, +X, all, a,b. edges: faces, a&b, x|y",
                    ),
                }
            },
        );
    }

    fn inspector(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.line_card(ui);
                if self.section.is_some() {
                    self.section_card(ui);
                }
                if self.measuring {
                    self.measure_card(ui);
                }
                self.parts_card(ui);
                if !self.shown.as_ref().is_some_and(|s| s.assembly) {
                    self.select_card(ui);
                }
            });
    }

    fn cut(&self) -> Option<Cut> {
        let (axis, at, flip) = self.section?;
        let scene = &self.shown.as_ref()?.scene;
        let low = scene.centre[axis] - scene.radius;
        let place = low + 2.0 * scene.radius * at;
        let mut normal = [0.0f32; 3];
        normal[axis] = if flip { -1.0 } else { 1.0 };
        Some(Cut {
            normal,
            offset: if flip { -place } else { place },
        })
    }

    fn section_card(&mut self, ui: &mut Ui) {
        let Some((mut axis, mut at, mut flip)) = self.section else {
            return;
        };
        let place = self.shown.as_ref().map(|shown| {
            let scene = &shown.scene;
            scene.centre[axis] - scene.radius + 2.0 * scene.radius * at
        });
        card(
            ui,
            None,
            |ui| {
                Line::new().legend("section").show(ui);
            },
            |ui| {
                ui.horizontal(|ui| {
                    for (k, name) in ["x", "y", "z"].iter().enumerate() {
                        if toggle(ui, name, axis == k).clicked() {
                            axis = k;
                        }
                    }
                    if toggle(ui, "flip", flip).clicked() {
                        flip = !flip;
                    }
                });
                ui.add(egui::Slider::new(&mut at, 0.0..=1.0).show_value(false));
                if let Some(place) = place {
                    note(
                        ui,
                        format!(
                            "cut at {}={place:.2}, showing {}",
                            ["x", "y", "z"][axis],
                            if flip { "above" } else { "below" }
                        ),
                        LEGEND,
                    );
                }
            },
        );
        ui.add_space(8.0);
        self.section = Some((axis, at, flip));
    }

    fn pick(
        &self,
        projector: &Projector,
        at: Pos2,
        placements: &[(Placement, bool)],
    ) -> Option<Pick> {
        let shown = self.shown.as_ref()?;
        let (origin, direction) = projector.ray(at);
        let visible = |body: usize| placements.get(body).is_none_or(|(_, v)| *v);
        let place = |body: usize, p: V3| placements.get(body).map_or(p, |(m, _)| apply(m, p));
        let cut = self.cut();
        let kept = |p: V3| cut.is_none_or(|c| c.keeps(p));
        let hit = shown
            .scene
            .surfaces
            .iter()
            .filter(|s| s.face.is_some() && visible(s.body))
            .flat_map(|s| {
                s.positions.chunks_exact(3).filter_map(move |t| {
                    let corners = [
                        place(s.body, t[0]),
                        place(s.body, t[1]),
                        place(s.body, t[2]),
                    ];
                    hit_triangle(origin, direction, corners)
                        .filter(|d| {
                            kept([
                                origin[0] + direction[0] * d,
                                origin[1] + direction[1] * d,
                                origin[2] + direction[2] * d,
                            ])
                        })
                        .map(|d| (d, s.face))
                })
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        let point = [
            origin[0] + direction[0] * hit.0,
            origin[1] + direction[1] * hit.0,
            origin[2] + direction[2] * hit.0,
        ];
        let corner = shown
            .scene
            .vertices
            .iter()
            .filter(|(body, _)| visible(*body))
            .map(|(body, p)| place(*body, *p))
            .filter(|p| kept(*p))
            .filter_map(|p| projector.project(p).map(|s| (s.distance(at), p)))
            .filter(|(d, p)| {
                *d < 10.0 && dot(sub(*p, point), sub(*p, point)).sqrt() < shown.scene.radius * 0.2
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        Some(match corner {
            Some((_, p)) => Pick {
                point: p,
                face: hit.1,
                vertex: true,
            },
            None => Pick {
                point,
                face: hit.1,
                vertex: false,
            },
        })
    }

    fn view_cube(&mut self, ui: &Ui, rect: Rect, projector: &Projector) -> bool {
        let size = 34.0;
        let centre = Pos2::new(rect.right() - 70.0, rect.top() + 70.0);
        let screen = |v: V3| {
            Pos2::new(
                centre.x + dot(v, projector.right) * size,
                centre.y - dot(v, projector.up) * size,
            )
        };
        let toward = [
            -projector.forward[0],
            -projector.forward[1],
            -projector.forward[2],
        ];
        let mut visible: Vec<(f32, Vec<Pos2>, V3, &str)> = CUBE_FACES
            .iter()
            .filter(|(n, _)| dot(*n, toward) > 0.05)
            .map(|(n, label)| {
                let (u, w) = if n[2].abs() > 0.5 {
                    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
                } else if n[0].abs() > 0.5 {
                    ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0])
                } else {
                    ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
                };
                let corner = |a: f32, b: f32| {
                    screen([
                        n[0] + u[0] * a + w[0] * b,
                        n[1] + u[1] * a + w[1] * b,
                        n[2] + u[2] * a + w[2] * b,
                    ])
                };
                let polygon = vec![
                    corner(-1.0, -1.0),
                    corner(1.0, -1.0),
                    corner(1.0, 1.0),
                    corner(-1.0, 1.0),
                ];
                (dot(*n, toward), polygon, *n, *label)
            })
            .collect();
        visible.sort_by(|a, b| a.0.total_cmp(&b.0));
        let area = Rect::from_center_size(centre, Vec2::splat(size * 3.6));
        let resp = ui.interact(area, ui.id().with("view cube"), Sense::click());
        let hover = resp.hover_pos();
        let p = ui.painter_at(rect);
        let mut chosen = None;
        for (facing, polygon, normal, label) in &visible {
            let lit = hover.is_some_and(|h| inside(polygon, h));
            let shade = (60.0 + 90.0 * facing) as u8;
            let fill = if lit {
                READOUT.gamma_multiply(0.55)
            } else {
                Color32::from_rgb(shade / 2 + 20, shade / 2 + 24, shade / 2 + 30)
            };
            p.add(egui::Shape::convex_polygon(
                polygon.clone(),
                fill,
                Stroke::new(1.0, ETCH),
            ));
            if *facing > 0.35 {
                let mid = polygon.iter().fold(Vec2::ZERO, |acc, q| acc + q.to_vec2()) / 4.0;
                p.text(
                    mid.to_pos2(),
                    egui::Align2::CENTER_CENTER,
                    *label,
                    egui::FontId::proportional(9.0),
                    VALUE,
                );
            }
            if resp.clicked() && lit {
                chosen = Some(*normal);
            }
        }
        if let Some(n) = chosen {
            self.cam.yaw = if n[2].abs() > 0.5 {
                0.0
            } else {
                n[0].atan2(-n[1])
            };
            self.cam.pitch = (n[2] * 1.5699).clamp(-1.5699, 1.5699);
        }
        let mut buttons_at = Pos2::new(centre.x - 62.0, centre.y + size * 1.9 + 4.0);
        for (name, yaw, pitch) in VIEWS {
            let button = Rect::from_min_size(buttons_at, Vec2::new(40.0, 16.0));
            let r = ui.interact(button, ui.id().with(("view", name)), Sense::click());
            let fill = if r.hovered() { BAND } else { PANEL };
            p.rect_filled(button, 2.0, fill);
            p.text(
                button.center(),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::proportional(10.0),
                if r.hovered() { READOUT } else { LEGEND },
            );
            if r.clicked() {
                self.cam.yaw = yaw;
                self.cam.pitch = pitch;
                if name == "iso" {
                    self.cam.zoom = 1.0;
                    self.cam.pan = Vec2::ZERO;
                }
            }
            buttons_at.x += 42.0;
            if buttons_at.x > centre.x + 60.0 {
                buttons_at = Pos2::new(centre.x - 62.0, buttons_at.y + 18.0);
            }
        }
        resp.hovered() || resp.clicked()
    }

    fn viewport(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let p = ui.painter_at(rect);
        let Some(shown) = &self.shown else {
            p.rect_filled(rect, 0.0, BACKGROUND);
            let message = match (
                &self.path,
                self.loading.is_some() || self.building.is_some(),
            ) {
                (None, _) => "open a .lcad part or .lasm assembly",
                (_, true) => "building",
                _ => "no solid at this line",
            };
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                message,
                egui::FontId::proportional(13.0),
                LEGEND,
            );
            return;
        };
        let scene = shown.scene.clone();
        let placements = Arc::new(self.placements());
        gl::paint(
            ui,
            rect,
            scene.clone(),
            self.cam,
            placements.clone(),
            self.cut(),
        );
        let projector = Projector::new(&scene, &self.cam, rect);
        let on_cube = self.view_cube(ui, rect, &projector);
        if !on_cube {
            if resp.dragged_by(egui::PointerButton::Primary) && !ui.input(|i| i.modifiers.shift) {
                let d = resp.drag_delta();
                self.cam.yaw -= d.x * 0.01;
                self.cam.pitch = (self.cam.pitch + d.y * 0.01).clamp(-1.5699, 1.5699);
            } else if resp.dragged() {
                self.cam.pan += resp.drag_delta();
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                self.cam.zoom = (self.cam.zoom * (1.0 + scroll * 0.002)).clamp(0.1, 40.0);
            }
            if resp.double_clicked() {
                self.cam = Camera {
                    ortho: self.cam.ortho,
                    ..Camera::default()
                };
            } else if self.measuring
                && resp.clicked()
                && let Some(at) = resp.interact_pointer_pos()
                && let Some(pick) = self.pick(&projector, at, &placements)
            {
                if self.picks.len() >= 2 {
                    self.picks.clear();
                }
                self.picks.push(pick);
            }
        }
        let projector = Projector::new(&scene, &self.cam, rect);
        let points: Vec<Pos2> = self
            .picks
            .iter()
            .filter_map(|pick| projector.project(pick.point))
            .collect();
        if let [a, b] = points[..] {
            p.line_segment([a, b], Stroke::new(1.5, TRACE));
            let d = sub(self.picks[1].point, self.picks[0].point);
            p.text(
                a.lerp(b, 0.5) + Vec2::new(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.3}", dot(d, d).sqrt()),
                egui::FontId::proportional(13.0),
                TRACE,
            );
        }
        for (n, at) in points.iter().enumerate() {
            p.circle_filled(*at, 4.0, if n == 0 { READOUT } else { TRACE });
            p.circle_stroke(*at, 4.0, Stroke::new(1.0, Color32::BLACK));
        }
        let help = if self.measuring {
            "click to pick points, corners snap; drag to orbit, shift-drag to pan, scroll to zoom"
        } else {
            "drag to orbit, shift-drag to pan, scroll to zoom, double-click to reset, up/down to step lines"
        };
        p.text(
            rect.left_bottom() + Vec2::new(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            help,
            egui::FontId::proportional(11.0),
            Color32::from_gray(130),
        );
        if self.loading.is_some() || self.building.is_some() {
            p.text(
                rect.left_top() + Vec2::new(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                "rebuilding",
                egui::FontId::proportional(11.0),
                READOUT,
            );
        }
    }

    fn browser(&mut self, ctx: &egui::Context) {
        let Some(browser) = &mut self.browser else {
            return;
        };
        let mut chosen: Option<PathBuf> = None;
        let mut close = false;
        let mut go: Option<PathBuf> = None;
        let mut open = true;
        egui::Window::new("open a part or assembly")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([560.0, 420.0])
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                Line::new()
                    .legend("folder")
                    .value(browser.dir.display().to_string())
                    .show(ui);
                let (entered, pressed) = ui
                    .horizontal(|ui| {
                        let typed = ui.add(
                            egui::TextEdit::singleline(&mut browser.typed)
                                .hint_text("path to a file or folder")
                                .desired_width(480.0),
                        );
                        let entered =
                            typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        (entered, toggle(ui, "go", false).clicked())
                    })
                    .inner;
                if entered || pressed {
                    let target = PathBuf::from(browser.typed.trim());
                    let target = if target.is_relative() {
                        browser.dir.join(target)
                    } else {
                        target
                    };
                    if target.is_dir() {
                        go = Some(target);
                    } else if target.is_file() {
                        chosen = Some(target);
                    }
                }
                ui.add_space(6.0);
                let mut entries: Vec<(bool, PathBuf)> = std::fs::read_dir(&browser.dir)
                    .map(|read| {
                        read.filter_map(|e| e.ok().map(|e| e.path()))
                            .filter(|p| {
                                let hidden = p
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .is_some_and(|n| n.starts_with('.'));
                                !hidden
                                    && (p.is_dir() || p.extension().is_some_and(|e| e == "lcad"))
                            })
                            .map(|p| (p.is_dir(), p))
                            .collect()
                    })
                    .unwrap_or_default();
                entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(320.0)
                    .show(ui, |ui| {
                        if let Some(parent) = browser.dir.parent()
                            && ui
                                .selectable_label(false, egui::RichText::new("..").monospace())
                                .clicked()
                        {
                            go = Some(parent.to_path_buf());
                        }
                        for (is_dir, path) in &entries {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or_default();
                            let text = if *is_dir {
                                format!("{name}/")
                            } else {
                                name.to_string()
                            };
                            let colour = if *is_dir { LEGEND } else { VALUE };
                            if ui
                                .selectable_label(
                                    false,
                                    egui::RichText::new(text).monospace().color(colour),
                                )
                                .clicked()
                            {
                                if *is_dir {
                                    go = Some(path.clone());
                                } else {
                                    chosen = Some(path.clone());
                                }
                            }
                        }
                        if entries.is_empty() {
                            hint(ui, "no folders, .lcad or .lasm files here");
                        }
                    });
                ui.add_space(6.0);
                if toggle(ui, "cancel", false).clicked() {
                    close = true;
                }
            });
        if let Some(dir) = go {
            browser.dir = dir;
            browser.typed.clear();
        }
        if !open || close {
            self.browser = None;
        }
        if let Some(path) = chosen {
            self.browser = None;
            self.open(path, ctx);
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.picks.clear();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.browser = Some(Browser::at(std::env::current_dir().unwrap_or_default()));
        }
        let count = self.program.lines.len();
        if count == 0 {
            return;
        }
        let (up, down, home, end) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
            )
        });
        if up && self.selected > 0 {
            self.select(self.selected - 1);
        }
        if down && self.selected + 1 < count {
            self.select(self.selected + 1);
        }
        if home {
            self.select(0);
        }
        if end {
            self.select(count - 1);
        }
    }
}

impl Browser {
    fn at(dir: PathBuf) -> Browser {
        Browser {
            dir,
            typed: String::new(),
        }
    }
}

fn build_shown(snapshot: &Snapshot, key: SceneKey) -> Option<Shown> {
    let model = &snapshot.model;
    if model.solids().is_empty() {
        return None;
    }
    let tolerance = model.tolerance();
    let own: Vec<usize> = model
        .solid
        .as_ref()
        .map(|solid| {
            select::select_faces(&label_of(&snapshot.line), solid, &model.groups, tolerance)
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let (query_faces, query_edges, query) = match (key.query.as_str(), model.solid.as_ref()) {
        ("", _) | (_, None) => (Vec::new(), Vec::new(), None),
        (text, Some(solid)) => {
            let faces = if text.contains('&') || text.contains('|') {
                Ok(Vec::new())
            } else {
                select::select_faces(text, solid, &model.groups, tolerance)
            };
            let edges = select::select_edges(text, solid, &model.groups, tolerance);
            match (faces, edges) {
                (Ok(faces), Ok(edges)) => {
                    let counts = (faces.len(), edges.len());
                    (faces, edges, Some(Ok(counts)))
                }
                (Err(error), _) | (_, Err(error)) => {
                    (Vec::new(), Vec::new(), Some(Err(format!("{error:#}"))))
                }
            }
        }
    };
    let rgb = |c: Color32| [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0);
    let highlights = [
        Highlight {
            faces: &own,
            colour: rgb(READOUT),
        },
        Highlight {
            faces: &query_faces,
            colour: rgb(TRACE),
        },
    ];
    let scene = scene::build(model, &highlights, &query_edges, rgb(TRACE));
    let bounds = model
        .solids()
        .iter()
        .map(|s| geometry::bounds(s))
        .reduce(|a, b| a + b)?;
    let (min, max) = (bounds.min(), bounds.max());
    let solids: Vec<(String, Solid)> = model
        .bodies
        .iter()
        .cloned()
        .chain(model.solid.clone().map(|s| (model.current_body(), s)))
        .collect();
    Some(Shown {
        key,
        scene: Arc::new(scene),
        faces: model.solids().iter().map(|s| select::faces(s).len()).sum(),
        bounds: ([min.x, min.y, min.z], [max.x, max.y, max.z]),
        query,
        joints: model.joints.clone(),
        rig: model.rig(),
        explode: model.explode.clone(),
        solids,
        assembly: model.assembly,
    })
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.keys(&ctx);
        self.refresh_scene(&ctx);
        egui::Panel::top("header")
            .frame(egui::Frame::NONE.fill(CHASSIS).inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 8,
                bottom: 2,
            }))
            .show(ui, |ui| self.header(ui, &ctx));
        egui::Panel::left("lines")
            .resizable(true)
            .default_size(360.0)
            .min_size(220.0)
            .frame(
                egui::Frame::NONE
                    .fill(CHASSIS)
                    .inner_margin(egui::Margin::symmetric(6, 8)),
            )
            .show(ui, |ui| self.lines(ui));
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(320.0)
            .min_size(260.0)
            .frame(
                egui::Frame::NONE
                    .fill(CHASSIS)
                    .inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter()
                .line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, ETCH));
            self.viewport(ui);
        });
        self.browser(&ctx);
    }
}

pub fn run(
    path: Option<PathBuf>,
    query: String,
    line: Option<usize>,
    vars: Vec<(String, f64)>,
) -> eframe::Result<()> {
    let title = match &path {
        Some(path) => format!("linecad - {}", path.display()),
        None => "linecad".to_string(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_title(title.clone()),
        depth_buffer: 24,
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, path, query, line, vars)))),
    )
}

fn exploded(
    moved: Matrix4,
    offset: Option<&monstertruck::modeling::Vector3>,
    scale: f64,
) -> Matrix4 {
    match offset {
        Some(by) => Matrix4::from_translation(by * scale) * moved,
        None => moved,
    }
}

#[cfg(test)]
mod tests {
    use monstertruck::modeling::*;

    #[test]
    fn explode_offsets_do_not_turn_with_the_part() {
        let turned = Matrix4::from_angle_z(Deg(90.0));
        let at = super::exploded(turned, Some(&Vector3::new(10.0, 0.0, 0.0)), 1.0)
            .transform_point(Point3::origin());
        assert!(
            (at - Point3::new(10.0, 0.0, 0.0)).magnitude() < 1.0e-9,
            "{at:?}"
        );
    }
}
