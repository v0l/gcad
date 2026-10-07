use super::gl::{self, BACKGROUND, Camera};
use super::scene::{self, Highlight, Scene};
use crate::geometry;
use crate::model::{Snapshot, label_of, run_snapshots_in};
use crate::parse::{Line as SourceLine, parse_program};
use crate::select;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_bench::prelude::*;
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

fn evaluate(path: &PathBuf) -> Program {
    match std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|s| parse_program(&s).map_err(|e| format!("{e:#}")))
    {
        Ok(lines) => Program {
            snapshots: run_snapshots_in(path.parent().map(std::path::Path::to_path_buf), &lines),
            lines,
            error: None,
        },
        Err(error) => Program {
            error: Some(error),
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
    volume: f64,
    faces: usize,
    bounds: ([f64; 3], [f64; 3]),
    query: Option<Result<(usize, usize), String>>,
}

enum Status {
    Ok,
    Failed,
    NotRun,
}

pub struct App {
    path: PathBuf,
    program: Arc<Program>,
    generation: u64,
    selected: usize,
    follow: bool,
    query: String,
    shown: Option<Shown>,
    cam: Camera,
    events: Option<Receiver<()>>,
    _watcher: Option<notify::RecommendedWatcher>,
    dirty: Option<Instant>,
    loading: Option<(Receiver<Program>, Instant)>,
    loaded: Instant,
    took: Duration,
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

impl App {
    fn new(
        cc: &eframe::CreationContext,
        path: PathBuf,
        query: String,
        line: Option<usize>,
    ) -> Self {
        egui_bench::install(&cc.egui_ctx);
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let mut app = App {
            path,
            program: Arc::new(Program::default()),
            generation: 0,
            selected: line.map_or(0, |n| n.saturating_sub(1)),
            follow: line.is_none(),
            query,
            shown: None,
            cam: Camera::default(),
            events: None,
            _watcher: None,
            dirty: None,
            loading: None,
            loaded: Instant::now(),
            took: Duration::ZERO,
        };
        app.reload(&cc.egui_ctx);
        app.watch(cc.egui_ctx.clone());
        app
    }

    fn watch(&mut self, ctx: egui::Context) {
        let (tx, rx) = channel();
        let file = self.path.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && changes_content(&event.kind)
                && event.paths.iter().any(|p| p == &file)
            {
                let _ = tx.send(());
                ctx.request_repaint();
            }
        });
        if let Ok(mut watcher) = watcher
            && let Some(dir) = self.path.parent()
            && watcher.watch(dir, RecursiveMode::NonRecursive).is_ok()
        {
            self._watcher = Some(watcher);
            self.events = Some(rx);
        }
    }

    fn reload(&mut self, ctx: &egui::Context) {
        let (tx, rx) = channel();
        let (path, ctx) = (self.path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(evaluate(&path));
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

    fn install(&mut self, program: Program) {
        self.generation += 1;
        self.loaded = Instant::now();
        let last = program.snapshots.len().saturating_sub(1);
        if self.follow || self.selected >= program.lines.len() {
            self.selected = last;
        }
        self.program = Arc::new(program);
    }

    fn status(&self, index: usize) -> Status {
        match self.program.snapshots.get(index).map(|s| s.result.is_ok()) {
            Some(true) => Status::Ok,
            Some(false) => Status::Failed,
            None => Status::NotRun,
        }
    }

    fn shown_snapshot(&self) -> Option<&Snapshot> {
        let snapshots = &self.program.snapshots;
        snapshots.get(self.selected.min(snapshots.len().saturating_sub(1)))
    }

    fn refresh_scene(&mut self) {
        let key = SceneKey {
            generation: self.generation,
            line: self.selected,
            query: self.query.trim().to_string(),
        };
        if self.shown.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let Some(snapshot) = self.shown_snapshot() else {
            self.shown = None;
            return;
        };
        let model = &snapshot.model;
        let Some(solid) = model.solid.as_ref() else {
            self.shown = None;
            return;
        };
        let tolerance = model.tolerance();
        let own: Vec<usize> =
            select::select_faces(&label_of(&snapshot.line), solid, &model.groups, tolerance)
                .unwrap_or_default();
        let (query_faces, query_edges, query) = match key.query.as_str() {
            "" => (Vec::new(), Vec::new(), None),
            text => {
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
        let others: Vec<&monstertruck::modeling::Solid> =
            model.bodies.iter().map(|(_, s)| s).collect();
        let scene = scene::build(solid, &others, &highlights, &query_edges, rgb(TRACE));
        let bounds = geometry::bounds(solid);
        let (min, max) = (bounds.min(), bounds.max());
        self.shown = Some(Shown {
            key,
            scene: Arc::new(scene),
            volume: geometry::volume(solid),
            faces: select::faces(solid).len(),
            bounds: ([min.x, min.y, min.z], [max.x, max.y, max.z]),
            query,
        });
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            Line::new()
                .legend("linecad")
                .value(self.path.display().to_string())
                .show(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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

    fn inspector(&mut self, ui: &mut Ui) {
        let program = self.program.clone();
        if let Some(line) = program.lines.get(self.selected) {
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
        if let Some(shown) = &self.shown {
            let (min, max) = shown.bounds;
            card(
                ui,
                None,
                |ui| {
                    Line::new().legend("solid").show(ui);
                },
                |ui| {
                    readouts(
                        ui,
                        &[
                            ("volume", format!("{:.3}", shown.volume), TRACE),
                            ("faces", shown.faces.to_string(), VALUE),
                            ("size x", format!("{:.3}", max[0] - min[0]), VALUE),
                            ("size y", format!("{:.3}", max[1] - min[1]), VALUE),
                            ("size z", format!("{:.3}", max[2] - min[2]), VALUE),
                        ],
                    );
                    Line::new()
                        .legend("min")
                        .value(format!("{:.2}, {:.2}, {:.2}", min[0], min[1], min[2]))
                        .size(11.0)
                        .show(ui);
                    Line::new()
                        .legend("max")
                        .value(format!("{:.2}, {:.2}, {:.2}", max[0], max[1], max[2]))
                        .size(11.0)
                        .show(ui);
                },
            );
            ui.add_space(8.0);
        }
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

    fn viewport(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        if resp.dragged_by(egui::PointerButton::Primary) && !ui.input(|i| i.modifiers.shift) {
            let d = resp.drag_delta();
            self.cam.yaw -= d.x * 0.01;
            self.cam.pitch = (self.cam.pitch + d.y * 0.01).clamp(-1.55, 1.55);
        } else if resp.dragged() {
            self.cam.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.cam.zoom = (self.cam.zoom * (1.0 + scroll * 0.002)).clamp(0.1, 40.0);
        }
        if resp.double_clicked() {
            self.cam = Camera::default();
        }
        let p = ui.painter_at(rect);
        match &self.shown {
            Some(shown) => gl::paint(ui, rect, shown.scene.clone(), self.cam),
            None => {
                p.rect_filled(rect, 0.0, BACKGROUND);
                let message = if self.loading.is_some() {
                    "building"
                } else {
                    "no solid at this line"
                };
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    message,
                    egui::FontId::proportional(13.0),
                    LEGEND,
                );
            }
        }
        p.text(
            rect.left_bottom() + Vec2::new(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            "drag to orbit, shift-drag to pan, scroll to zoom, double-click to reset, up/down to step lines",
            egui::FontId::proportional(11.0),
            Color32::from_gray(130),
        );
        if self.loading.is_some() && self.shown.is_some() {
            p.text(
                rect.right_top() + Vec2::new(-10.0, 10.0),
                egui::Align2::RIGHT_TOP,
                "rebuilding",
                egui::FontId::proportional(11.0),
                READOUT,
            );
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.keys(&ctx);
        self.refresh_scene();
        egui::Panel::top("header")
            .frame(egui::Frame::NONE.fill(CHASSIS).inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 8,
                bottom: 2,
            }))
            .show(ui, |ui| self.header(ui));
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
            .default_size(300.0)
            .min_size(240.0)
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
    }
}

pub fn run(path: PathBuf, query: String, line: Option<usize>) -> eframe::Result<()> {
    let title = format!("linecad - {}", path.display());
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
        Box::new(move |cc| Ok(Box::new(App::new(cc, path, query, line)))),
    )
}
