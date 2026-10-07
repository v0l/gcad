use super::scene::{Scene, V3};
use eframe::egui_glow;
use egui::{Color32, Rect, Ui, Vec2};
use std::cell::RefCell;
use std::sync::Arc;
use three_d::*;

pub const BACKGROUND: Color32 = Color32::from_rgb(20, 22, 26);
const AMBIENT: f32 = 0.32;
const KEY: f32 = 0.78;
const FILL: f32 = 0.25;

#[derive(Clone, Copy, PartialEq)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: Vec2,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            yaw: 0.6,
            pitch: 0.55,
            zoom: 1.0,
            pan: Vec2::ZERO,
        }
    }
}

pub struct View {
    pub eye: V3,
    pub target: V3,
    pub up: V3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    pub key: V3,
    pub fill: V3,
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: V3) -> V3 {
    scale(a, 1.0 / dot(a, a).sqrt().max(1.0e-12))
}

pub fn view(scene: &Scene, cam: &Camera, size: Vec2) -> View {
    let (cy, sy, cp, sp) = (
        cam.yaw.cos(),
        cam.yaw.sin(),
        cam.pitch.cos(),
        cam.pitch.sin(),
    );
    let eye_dir = [sy * cp, -cy * cp, sp];
    let dist = scene.radius * 2.6;
    let forward = norm(scale(eye_dir, -1.0));
    let right = norm(cross(forward, [0.0, 0.0, 1.0]));
    let right = if dot(right, right) < 0.5 {
        [1.0, 0.0, 0.0]
    } else {
        right
    };
    let up = cross(right, forward);
    let focal = size.x.min(size.y) * 1.25 * cam.zoom;
    let shift = add(
        scale(right, -cam.pan.x * dist / focal),
        scale(up, cam.pan.y * dist / focal),
    );
    let target = add(scene.centre, shift);
    let eye = add(target, scale(eye_dir, dist));
    let back = scale(forward, -1.0);
    View {
        eye,
        target,
        up,
        fov_y: 2.0 * (size.y * 0.5 / focal).atan(),
        near: dist * 0.02,
        far: dist * 10.0,
        key: norm(add(
            add(scale(right, -0.35), scale(up, 0.55)),
            scale(back, 0.75),
        )),
        fill: norm(add(
            add(scale(right, 0.55), scale(up, -0.25)),
            scale(back, 0.8),
        )),
    }
}

struct Shaded {
    colour: Vec3,
    eye: Vec3,
    key: Vec3,
    fill: Vec3,
}

impl Material for Shaded {
    fn id(&self) -> EffectMaterialId {
        EffectMaterialId(0x7c01)
    }

    fn fragment_shader_source(&self, _lights: &[&dyn Light]) -> String {
        format!(
            "const float AMBIENT = {AMBIENT:.4};\nconst float KEY = {KEY:.4};\nconst float FILL = {FILL:.4};\n{}",
            r#"
uniform vec3 surfaceColour;
uniform vec3 eye;
uniform vec3 keyDir;
uniform vec3 fillDir;
in vec3 pos;
in vec3 nor;
layout (location = 0) out vec4 outColor;

vec3 to_linear(vec3 c) {
    return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(vec3(0.04045), c));
}

vec3 to_srgb(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), c));
}

void main() {
    vec3 albedo = to_linear(surfaceColour);
    vec3 n = normalize(nor);
    vec3 v = normalize(eye - pos);
    if (dot(n, v) < 0.0) n = -n;
    float d = AMBIENT + KEY * max(dot(n, keyDir), 0.0) + FILL * max(dot(n, fillDir), 0.0);
    vec3 h = normalize(keyDir + v);
    float sp = 0.12 * pow(max(dot(n, h), 0.0), 40.0);
    outColor = vec4(to_srgb(albedo * d + vec3(sp)), 1.0);
}
"#
        )
    }

    fn use_uniforms(&self, program: &Program, _viewer: &dyn Viewer, _lights: &[&dyn Light]) {
        program.use_uniform_if_required("surfaceColour", self.colour);
        program.use_uniform_if_required("eye", self.eye);
        program.use_uniform_if_required("keyDir", self.key);
        program.use_uniform_if_required("fillDir", self.fill);
    }

    fn render_states(&self) -> RenderStates {
        RenderStates {
            cull: Cull::None,
            ..Default::default()
        }
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Opaque
    }
}

struct Gpu {
    context: Context,
    scene: u64,
    objects: Vec<Gm<Mesh, Shaded>>,
}

impl Gpu {
    fn upload(&mut self, scene: &Scene) {
        let v3 = |v: &[f32; 3]| vec3(v[0], v[1], v[2]);
        self.objects = scene
            .surfaces
            .iter()
            .filter(|s| !s.positions.is_empty())
            .map(|s| {
                let cpu = CpuMesh {
                    positions: Positions::F32(s.positions.iter().map(v3).collect()),
                    normals: Some(s.normals.iter().map(v3).collect()),
                    ..Default::default()
                };
                let material = Shaded {
                    colour: v3(&s.colour),
                    eye: vec3(0.0, 0.0, 1.0),
                    key: vec3(0.0, 0.0, 1.0),
                    fill: vec3(0.0, 0.0, 1.0),
                };
                Gm::new(Mesh::new(&self.context, &cpu), material)
            })
            .collect();
        self.scene = scene.id;
    }
}

thread_local! {
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
}

pub fn paint(ui: &Ui, rect: Rect, scene: Arc<Scene>, cam: Camera) {
    let callback = egui_glow::CallbackFn::new(move |info, painter| {
        GPU.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                let Ok(context) = Context::from_gl_context(painter.gl().clone()) else {
                    return;
                };
                *slot = Some(Gpu {
                    context,
                    scene: 0,
                    objects: Vec::new(),
                });
            }
            let Some(gpu) = slot.as_mut() else { return };
            if gpu.scene != scene.id {
                gpu.upload(&scene);
            }
            let vp = info.viewport_in_pixels();
            let clip = info.clip_rect_in_pixels();
            let vw = view(
                &scene,
                &cam,
                egui::Vec2::new(vp.width_px as f32, vp.height_px as f32),
            );
            let v3 = |v: [f32; 3]| vec3(v[0], v[1], v[2]);
            let viewport = Viewport {
                x: vp.left_px,
                y: vp.from_bottom_px,
                width: vp.width_px.max(1) as u32,
                height: vp.height_px.max(1) as u32,
            };
            let camera = three_d::Camera::new_perspective(
                viewport,
                v3(vw.eye),
                v3(vw.target),
                v3(vw.up),
                radians(vw.fov_y),
                vw.near,
                vw.far,
            );
            gpu.objects.iter_mut().for_each(|gm| {
                gm.material.eye = v3(vw.eye);
                gm.material.key = v3(vw.key);
                gm.material.fill = v3(vw.fill);
            });
            let x0 = clip.left_px.max(vp.left_px);
            let y0 = clip.from_bottom_px.max(vp.from_bottom_px);
            let x1 = (clip.left_px + clip.width_px).min(vp.left_px + vp.width_px);
            let y1 = (clip.from_bottom_px + clip.height_px).min(vp.from_bottom_px + vp.height_px);
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            let scissor = ScissorBox {
                x: x0,
                y: y0,
                width: (x1 - x0) as u32,
                height: (y1 - y0) as u32,
            };
            let [w, h] = info.screen_size_px;
            let target = match painter.intermediate_fbo() {
                Some(fbo) => RenderTarget::from_framebuffer(&gpu.context, w, h, fbo),
                None => RenderTarget::screen(&gpu.context, w, h),
            };
            let bg = BACKGROUND.to_array().map(|c| c as f32 / 255.0);
            target.clear_partially(
                scissor,
                ClearState::color_and_depth(bg[0], bg[1], bg[2], 1.0, 1.0),
            );
            target.render_partially(scissor, &camera, gpu.objects.iter(), &[]);
            let _ = target.into_framebuffer();
        });
    });
    ui.painter_at(rect).add(egui::PaintCallback {
        rect,
        callback: Arc::new(callback),
    });
}
