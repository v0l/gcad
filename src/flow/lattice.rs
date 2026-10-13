use super::gpu::{Gpu, Reading};
use super::rotor::{COVERED, Rotor, SUBCELLS};
use anyhow::{Result, bail};
use wgpu::util::DeviceExt;

pub const Q: usize = 19;

pub const C: [[i32; 3]; Q] = [
    [0, 0, 0],
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
    [1, 1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, -1],
    [1, -1, 0],
    [-1, 1, 0],
    [1, 0, -1],
    [-1, 0, 1],
    [0, 1, -1],
    [0, -1, 1],
];

pub fn weight(i: usize) -> f64 {
    match i {
        0 => 1.0 / 3.0,
        1..=6 => 1.0 / 18.0,
        _ => 1.0 / 36.0,
    }
}

pub fn opposite(i: usize) -> usize {
    match i {
        0 => 0,
        i if i % 2 == 1 => i + 1,
        i => i - 1,
    }
}

pub const FLUID: u8 = 0;
pub const WALL: u8 = 1;
pub const MOVING: u8 = 2;
pub const INLET: u8 = 3;
pub const OUTLET: u8 = 4;

pub struct Domain {
    pub size: [usize; 3],
    pub flags: Vec<u8>,
    pub body: Vec<u16>,
    pub inflow: Vec<(usize, usize, [f64; 3])>,
    pub outflow: Vec<(usize, usize)>,
    pub cuts: Vec<(usize, usize, f64)>,
    pub half: bool,
    pub heat: Option<Heat>,
    pub rotors: Vec<Rotor>,
}

pub struct Heat {
    pub cells: Vec<[f32; 2]>,
    pub probes: Vec<usize>,
}

impl Domain {
    pub fn new(size: [usize; 3]) -> Domain {
        let count = size.iter().product();
        Domain {
            size,
            flags: vec![FLUID; count],
            body: vec![0; count],
            inflow: Vec::new(),
            outflow: Vec::new(),
            cuts: Vec::new(),
            half: false,
            heat: None,
            rotors: Vec::new(),
        }
    }

    pub fn at(&self, x: usize, y: usize, z: usize) -> usize {
        x + (y + z * self.size[1]) * self.size[0]
    }

    pub fn close_diagonals(&mut self) -> usize {
        use rayon::prelude::*;
        let [nx, ny, nz] = self.size;
        let diagonals: Vec<[i32; 3]> = C
            .iter()
            .skip(1)
            .filter(|c| c.iter().filter(|v| **v != 0).count() == 2)
            .copied()
            .collect();
        let mut closed = 0;
        loop {
            let flags = &self.flags;
            let neighbour = |cell: usize, c: [i32; 3]| -> Option<usize> {
                let (x, y, z) = (cell % nx, (cell / nx) % ny, cell / (nx * ny));
                let step = |v: usize, d: i32, n: usize| {
                    let w = v as i64 + d as i64;
                    (0..n as i64).contains(&w).then_some(w as usize)
                };
                Some(step(x, c[0], nx)? + (step(y, c[1], ny)? + step(z, c[2], nz)? * ny) * nx)
            };
            let candidates: Vec<(usize, usize, usize)> = (0..flags.len())
                .into_par_iter()
                .filter(|&cell| flags[cell] == FLUID)
                .flat_map_iter(|cell| {
                    diagonals.iter().filter_map(move |c| {
                        let far = neighbour(cell, *c)?;
                        if flags[far] != FLUID {
                            return None;
                        }
                        let sides: Vec<usize> = (0..3)
                            .filter(|&d| c[d] != 0)
                            .map(|d| {
                                let mut part = [0; 3];
                                part[d] = c[d];
                                neighbour(cell, part)
                            })
                            .collect::<Option<_>>()?;
                        sides
                            .iter()
                            .all(|&s| flags[s] == WALL)
                            .then_some((cell, far, sides[0]))
                    })
                })
                .collect();
            let mut changed = 0;
            for (cell, far, side) in candidates {
                if self.flags[cell] == FLUID && self.flags[far] == FLUID {
                    self.flags[far] = WALL;
                    self.body[far] = self.body[side];
                    changed += 1;
                }
            }
            closed += changed;
            if changed == 0 {
                break;
            }
        }
        closed
    }

    pub fn seal(&mut self, from: &[usize]) -> usize {
        let [nx, ny, nz] = self.size;
        let open = |flag: u8| matches!(flag, FLUID | INLET | OUTLET);
        let links = |cell: usize| {
            let (x, y, z) = (cell % nx, (cell / nx) % ny, cell / (nx * ny));
            C.iter().skip(1).map(move |c| {
                let w = |v: usize, d: i32, n: usize| {
                    (v as i64 + d as i64).rem_euclid(n as i64) as usize
                };
                w(x, c[0], nx) + (w(y, c[1], ny) + w(z, c[2], nz) * ny) * nx
            })
        };
        let flags = &self.flags;
        let deep = |cell: usize| open(flags[cell]) && links(cell).all(|n| open(flags[n]));
        let mut reached = vec![false; flags.len()];
        let mut queue: Vec<usize> = from.iter().copied().filter(|&c| deep(c)).collect();
        for &cell in &queue {
            reached[cell] = true;
        }
        while let Some(cell) = queue.pop() {
            for next in links(cell) {
                if !reached[next] && deep(next) {
                    reached[next] = true;
                    queue.push(next);
                }
            }
        }
        let outside: Vec<bool> = (0..flags.len())
            .map(|cell| reached[cell] || (open(flags[cell]) && links(cell).any(|n| reached[n])))
            .collect();
        let mut sealed = 0;
        for (cell, flag) in self.flags.iter_mut().enumerate() {
            if *flag == FLUID && !outside[cell] {
                *flag = WALL;
                self.body[cell] = 0;
                sealed += 1;
            }
        }
        let flags = &self.flags;
        self.cuts.retain(|&(cell, ..)| flags[cell] == FLUID);
        sealed
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub viscosity: f64,
    pub force: [f64; 3],
    pub wall: [f64; 3],
    pub collision: Collision,
    pub smagorinsky: f64,
    pub inflow: f64,
    pub sponge: f64,
    pub stream: [f64; 3],
    pub bulk: f64,
    pub sponge_axis: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collision {
    Bgk,
    Regularized,
}

impl Settings {
    pub fn laminar(viscosity: f64) -> Settings {
        Settings {
            viscosity,
            force: [0.0; 3],
            wall: [0.0; 3],
            collision: Collision::Regularized,
            smagorinsky: 0.0,
            inflow: 1.0,
            sponge: 0.0,
            stream: [0.0; 3],
            bulk: 0.0,
            sponge_axis: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    nx: u32,
    ny: u32,
    nz: u32,
    count: u32,
    omega: f32,
    fx: f32,
    fy: f32,
    fz: f32,
    wx: f32,
    wy: f32,
    wz: f32,
    regular: f32,
    smagorinsky: f32,
    sponge: f32,
    bulk: f32,
    sx: f32,
    sy: f32,
    sz: f32,
    axis: u32,
}

fn neighbours(index: &str) -> String {
    let mut s = String::new();
    s += &format!(
        "  let x = {index} % p.nx; let row = {index} / p.nx; let y = row % p.ny; let z = row / p.ny;\n"
    );
    s += "  let xm = select(x - 1u, p.nx - 1u, x == 0u); let xp = select(x + 1u, 0u, x + 1u == p.nx);\n";
    s += "  let ym = select(y - 1u, p.ny - 1u, y == 0u) * p.nx; let y0 = y * p.nx; let yp = select(y + 1u, 0u, y + 1u == p.ny) * p.nx;\n";
    s += "  let layer = p.nx * p.ny;\n";
    s += "  let zm = select(z - 1u, p.nz - 1u, z == 0u) * layer; let z0 = z * layer; let zp = select(z + 1u, 0u, z + 1u == p.nz) * layer;\n";
    s
}

fn offset(c: &[i32; 3], sign: i32) -> String {
    let pick = |v: i32, names: [&'static str; 3]| names[(v * sign + 1) as usize];
    format!(
        "{} + {} + {}",
        pick(c[0], ["xm", "x", "xp"]),
        pick(c[1], ["ym", "y0", "yp"]),
        pick(c[2], ["zm", "z0", "zp"])
    )
}

pub const TQ: usize = 7;

fn thermal_populations(group: u32) -> String {
    let mut s = String::new();
    for i in 0..TQ {
        s +=
            &format!("@group({group}) @binding({i}) var<storage, read_write> ta{i}: array<f32>;\n");
        s += &format!(
            "@group({group}) @binding({}) var<storage, read_write> tb{i}: array<f32>;\n",
            i + TQ
        );
    }
    s
}

fn thermal_update(alpha: &str, source: &str, still: bool) -> String {
    let mut s = String::new();
    s += "  {\n";
    s += &format!("    let tm = 0.5 + 4.0 * ({alpha});\n");
    s += "    let wm = 1.0 / tm; let wp = 1.0 / (0.5 + 0.25 / (tm - 0.5));\n";
    s += &format!("    let tq = {source};\n");
    for (i, c) in C.iter().enumerate().take(TQ) {
        let o = opposite(i);
        s += &format!(
            "    var h{i}: f32; {{ let n = {}; let k = flags[n]; let kk = k & KIND; if ((kk == 1u || kk == 2u) && (k >> 8u) == 0u) {{ h{i} = ta{o}[index]; }} else if (kk == 3u) {{ h{i} = -ta{o}[index]; }} else {{ h{i} = ta{i}[n]; }} }}\n",
            offset(c, -1)
        );
    }
    s += "    let theta = h0 + h1 + h2 + h3 + h4 + h5 + h6;\n";
    s += "    tb0[index] = h0 - wp * (h0 - 0.25 * theta) + 0.25 * tq;\n";
    for i in (1..TQ).step_by(2) {
        let o = opposite(i);
        let c = C[i];
        let cu = if still {
            "0.0".to_string()
        } else {
            format!(
                "(({}.0) * ux + ({}.0) * uy + ({}.0) * uz)",
                c[0], c[1], c[2]
            )
        };
        s += &format!(
            "    {{ let plus = 0.5 * (h{i} + h{o}) - 0.125 * theta; let minus = 0.5 * (h{i} - h{o}) - 0.5 * theta * {cu}; tb{i}[index] = h{i} - wp * plus - wm * minus + 0.125 * tq; tb{o}[index] = h{o} - wp * plus + wm * minus + 0.125 * tq; }}\n"
        );
    }
    s += "  }\n";
    s
}

fn shader(heat: bool) -> String {
    let mut s = String::new();
    s += "const KIND: u32 = 255u;\n";
    s += "struct Params { nx: u32, ny: u32, nz: u32, count: u32, omega: f32, fx: f32, fy: f32, fz: f32, wx: f32, wy: f32, wz: f32, regular: f32, smagorinsky: f32, sponge: f32, bulk: f32, sx: f32, sy: f32, sz: f32, axis: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> p: Params;\n";
    s += "@group(0) @binding(1) var<storage, read> flags: array<u32>;\n";
    s += "@group(0) @binding(2) var<storage, read> cuts: array<f32>;\n";
    s += "@group(0) @binding(4) var<storage, read> spins: array<vec4<f32>>;\n";
    s += "fn wall_velocity(word: u32, at: vec3<f32>) -> vec3<f32> { let b = word >> 8u; if (b == 0u) { return vec3<f32>(p.wx, p.wy, p.wz); } return cross(spins[2u * b].xyz, at - spins[2u * b + 1u].xyz); }\n";
    for i in 0..Q {
        s += &format!("@group(1) @binding({i}) var<storage, read_write> a{i}: array<P>;\n");
        s += &format!("@group(2) @binding({i}) var<storage, read_write> b{i}: array<P>;\n");
    }
    if heat {
        s += "@group(0) @binding(3) var<storage, read> heat: array<vec2<f32>>;\n";
        s += &thermal_populations(3);
    }
    s += "@compute @workgroup_size(64)\n";
    s += "fn step(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n";
    s += "  let index = id.x + id.y * groups.x * 64u;\n";
    s += "  if (index >= p.count) { return; }\n";
    s += "  let own = flags[index];\n";
    if heat {
        s += &neighbours("index");
        s += "  let kind = own & KIND;\n";
        s += "  if (kind != 0u) {\n";
        s += "    if ((kind == 1u || kind == 2u) && (own >> 8u) != 0u) {\n";
        s += &thermal_update("heat[index].x", "heat[index].y", true);
        s += "    }\n    return;\n  }\n";
    } else {
        s += "  if ((own & KIND) != 0u) { return; }\n";
        s += &neighbours("index");
    }
    s += "  let slot = own >> 8u;\n";
    for i in 0..Q {
        s += &format!("  var g{i}: f32;\n");
    }
    s += "  var fs: u32;\n";
    for (i, c) in C.iter().enumerate() {
        let w = weight(i);
        let o = opposite(i);
        s += &format!("  let s{i} = {};\n", offset(c, -1));
        s += &format!("  let w{i} = flags[s{i}]; fs = w{i} & KIND;\n");
        s += &format!(
            "  g{i} = a{i}[s{i}];\n  if (fs == 1u || fs == 2u) {{ g{i} = a{o}[index]; if (slot > 0u) {{ let q = cuts[(slot - 1u) * 19u + {i}u]; if (q >= 0.0) {{ if (q < 0.5) {{ let n = {}; let kn = flags[n] & KIND; if (kn == 0u || kn == 3u || kn == 4u) {{ g{i} = 2.0 * q * a{o}[index] + (1.0 - 2.0 * q) * a{o}[n]; }} }} else {{ g{i} = (0.5 / q) * a{o}[index] + (1.0 - 0.5 / q) * a{i}[index]; }} }} }} if (fs == 2u) {{ let c = vec3<f32>({}.0, {}.0, {}.0); g{i} = g{i} + {:.17} * dot(c, wall_velocity(w{i}, vec3<f32>(f32(x), f32(y), f32(z)) - 0.5 * c)); }} }}\n",
            offset(c, 1),
            c[0],
            c[1],
            c[2],
            6.0 * w,
        );
    }
    let sum = (0..Q)
        .map(|i| format!("g{i}"))
        .collect::<Vec<_>>()
        .join(" + ");
    s += &format!("  let rho = 1.0 + {sum};\n");
    for (axis, name) in ["x", "y", "z"].iter().enumerate() {
        let terms: Vec<String> = C
            .iter()
            .enumerate()
            .filter(|(_, c)| c[axis] != 0)
            .map(|(i, c)| format!("{}g{i}", if c[axis] > 0 { "+" } else { "-" }))
            .collect();
        s += &format!("  let m{name} = 0.0 {};\n", terms.join(" "));
    }
    s += "  let ux = (mx + 0.5 * p.fx) / rho; let uy = (my + 0.5 * p.fy) / rho; let uz = (mz + 0.5 * p.fz) / rho;\n";
    s += "  let uu = ux * ux + uy * uy + uz * uz;\n";
    let pairs = [
        (0, 0, "xx"),
        (1, 1, "yy"),
        (2, 2, "zz"),
        (0, 1, "xy"),
        (0, 2, "xz"),
        (1, 2, "yz"),
    ];
    for (a, b, name) in pairs {
        let terms: Vec<String> = C
            .iter()
            .enumerate()
            .filter(|(_, c)| c[a] * c[b] != 0)
            .map(|(i, c)| format!("{}g{i}", if c[a] * c[b] > 0 { "+" } else { "-" }))
            .collect();
        let u = ["ux", "uy", "uz"];
        let iso = if a == b { " - (rho - 1.0) / 3.0" } else { "" };
        s += &format!(
            "  let p{name} = 0.0 {}{iso} - rho * {} * {} - 0.5 * ({} * p.f{} + {} * p.f{});\n",
            terms.join(" "),
            u[a],
            u[b],
            u[a],
            ["x", "y", "z"][b],
            u[b],
            ["x", "y", "z"][a]
        );
    }
    s += "  let pnorm = sqrt(pxx * pxx + pyy * pyy + pzz * pzz + 2.0 * (pxy * pxy + pxz * pxz + pyz * pyz));\n";
    s += "  let tau0 = 1.0 / p.omega;\n";
    s += "  let cs2 = p.smagorinsky * p.smagorinsky;\n";
    s += "  let tau = 0.5 * (tau0 + sqrt(tau0 * tau0 + 18.0 * 1.4142135 * cs2 * pnorm / rho));\n";
    s += "  let omega = 1.0 / tau;\n";
    s += "  let keep = 1.0 - 0.5 * omega;\n";
    s += "  let shed = select(0.0, (omega - 1.0 / (tau + 4.5 * p.bulk)) * (pxx + pyy + pzz) / 3.0, p.regular > 0.5);\n";
    s += "  let along = select(select(f32(x), f32(y), p.axis == 1u), f32(z), p.axis == 2u);\n";
    s += "  let span = select(select(f32(p.nx), f32(p.ny), p.axis == 1u), f32(p.nz), p.axis == 2u);\n";
    s += "  let edge = span - 1.0 - along;\n";
    s += "  var damp = 0.0;\n";
    s += "  if (p.sponge > 0.0 && edge < p.sponge) { let r = 1.0 - edge / p.sponge; damp = 0.5 * r * r; }\n";
    s += "  let su = p.sx * p.sx + p.sy * p.sy + p.sz * p.sz;\n";
    for (i, c) in C.iter().enumerate() {
        let w = weight(i);
        let q = |a: usize, b: usize| -> f64 {
            (c[a] * c[b]) as f64 - if a == b { 1.0 / 3.0 } else { 0.0 }
        };
        let projection = format!(
            "({:.17} * pxx + {:.17} * pyy + {:.17} * pzz + 2.0 * ({:.17} * pxy + {:.17} * pxz + {:.17} * pyz))",
            q(0, 0),
            q(1, 1),
            q(2, 2),
            q(0, 1),
            q(0, 2),
            q(1, 2)
        );
        s += &format!(
            "  {{ let cu = ({}.0) * ux + ({}.0) * uy + ({}.0) * uz;\n    let eq = {w:.17} * (rho - 1.0 + rho * (3.0 * cu + 4.5 * cu * cu - 1.5 * uu));\n    let push = keep * {w:.17} * ((3.0 * (({}.0) - ux) + 9.0 * cu * ({}.0)) * p.fx + (3.0 * (({}.0) - uy) + 9.0 * cu * ({}.0)) * p.fy + (3.0 * (({}.0) - uz) + 9.0 * cu * ({}.0)) * p.fz);\n    let free = select(g{i} - eq, {:.17} * {projection} - {w:.17} * 1.5 * (({sx}.0) * p.fx + ({sy}.0) * p.fy + ({sz}.0) * p.fz), p.regular > 0.5);\n    let next = eq + (1.0 - omega) * free + push + {:.17} * shed;\n    let cs = ({sx}.0) * p.sx + ({sy}.0) * p.sy + ({sz}.0) * p.sz;\n    let far = {w:.17} * (3.0 * cs + 4.5 * cs * cs - 1.5 * su);\n    b{i}[index] = next - damp * (next - far); }}\n",
            c[0],
            c[1],
            c[2],
            c[0],
            c[0],
            c[1],
            c[1],
            c[2],
            c[2],
            4.5 * w,
            4.5 * w * ((c[0] * c[0] + c[1] * c[1] + c[2] * c[2]) as f64 - 1.0),
            sx = c[0],
            sy = c[1],
            sz = c[2]
        );
    }
    if heat {
        s += &thermal_update("heat[index].x", "heat[index].y", false);
    }
    s += "}\n";
    s
}

fn populations(group: u32, name: &str) -> String {
    (0..Q)
        .map(|i| {
            format!("@group({group}) @binding({i}) var<storage, read_write> {name}{i}: array<P>;\n")
        })
        .collect()
}

fn plane_shader() -> String {
    let mut s = String::new();
    s += "struct Plane { nx: u32, ny: u32, nz: u32, axis: u32, index: u32, width: u32, count: u32, pad: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> p: Plane;\n";
    s += "@group(0) @binding(1) var<storage, read> flags: array<u32>;\n";
    s += "@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;\n";
    s += &populations(1, "a");
    s += "@compute @workgroup_size(64)\n";
    s += "fn plane(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n";
    s += "  let k = id.x + id.y * groups.x * 64u;\n";
    s += "  if (k >= p.count) { return; }\n";
    s += "  let u = k % p.width; let v = k / p.width;\n";
    s += "  var x = u; var y = v; var z = p.index;\n";
    s += "  if (p.axis == 0u) { x = p.index; y = u; z = v; } else if (p.axis == 1u) { x = u; y = p.index; z = v; }\n";
    s += "  let cell = x + (y + z * p.ny) * p.nx;\n";
    s += "  let kind = flags[cell] & 255u;\n";
    s += "  if (kind == 1u || kind == 2u) { out[k] = vec4<f32>(0.0, 0.0, 0.0, -1.0); return; }\n";
    let sum = (0..Q)
        .map(|i| format!("a{i}[cell]"))
        .collect::<Vec<_>>()
        .join(" + ");
    s += &format!("  let rho = 1.0 + {sum};\n");
    for (axis, name) in ["x", "y", "z"].iter().enumerate() {
        let terms: Vec<String> = C
            .iter()
            .enumerate()
            .filter(|(_, c)| c[axis] != 0)
            .map(|(i, c)| format!("{}a{i}[cell]", if c[axis] > 0 { "+" } else { "-" }))
            .collect();
        s += &format!("  let m{name} = (0.0 {}) / rho;\n", terms.join(" "));
    }
    s += "  out[k] = vec4<f32>(mx, my, mz, rho);\n";
    s += "}\n";
    s
}

pub const MEAN_CELLS: usize = 16_000_000;

pub fn mean_stride(size: [usize; 3]) -> usize {
    let cells = size.iter().product::<usize>() as f64;
    ((cells / MEAN_CELLS as f64).cbrt().ceil() as usize).max(1)
}

fn mean_shader(add: bool) -> String {
    let width = if add { 2 } else { 1 };
    let mut s = String::new();
    s += "struct Mean { nx: u32, ny: u32, nz: u32, stride: u32, cx: u32, cy: u32, cz: u32, count: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> m: Mean;\n";
    s += "@group(0) @binding(1) var<storage, read> flags: array<u32>;\n";
    s += "@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;\n";
    s += &populations(1, "a");
    s += "@compute @workgroup_size(64)\n";
    s += "fn mean(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n";
    s += "  let k = id.x + id.y * groups.x * 64u;\n";
    s += "  if (k >= m.count) { return; }\n";
    s += "  let bx = (k % m.cx) * m.stride; let by = ((k / m.cx) % m.cy) * m.stride; let bz = (k / (m.cx * m.cy)) * m.stride;\n";
    s += "  var total = vec4<f32>(0.0, 0.0, 0.0, 0.0);\n";
    s += "  var mass = 0.0;\n";
    s += "  for (var dz = 0u; dz < m.stride; dz++) { for (var dy = 0u; dy < m.stride; dy++) { for (var dx = 0u; dx < m.stride; dx++) {\n";
    s += "    let x = bx + dx; let y = by + dy; let z = bz + dz;\n";
    s += "    if (x >= m.nx || y >= m.ny || z >= m.nz) { continue; }\n";
    s += "    let cell = x + (y + z * m.ny) * m.nx;\n";
    s += "    let kind = flags[cell] & 255u;\n";
    s += "    if (kind == 1u || kind == 2u) { continue; }\n";
    let sum = (0..Q)
        .map(|i| format!("a{i}[cell]"))
        .collect::<Vec<_>>()
        .join(" + ");
    s += &format!("    let rho = 1.0 + {sum};\n");
    for (axis, name) in ["x", "y", "z"].iter().enumerate() {
        let terms: Vec<String> = C
            .iter()
            .enumerate()
            .filter(|(_, c)| c[axis] != 0)
            .map(|(i, c)| format!("{}a{i}[cell]", if c[axis] > 0 { "+" } else { "-" }))
            .collect();
        s += &format!("    let m{name} = (0.0 {}) / rho;\n", terms.join(" "));
    }
    s += "    total = total + vec4<f32>(mx, my, mz, 1.0);\n";
    s += "    mass = mass + rho;\n";
    s += "  } } }\n";
    s += &format!("  let at = k * {width}u;\n");
    s += if add {
        "  out[at] = out[at] + total;\n  out[at + 1u] = out[at + 1u] + vec4<f32>(mass, 0.0, 0.0, 0.0);\n"
    } else {
        "  out[at] = total;\n"
    };
    s += "}\n";
    s
}

fn edge_shader(heat: bool) -> String {
    let mut s = String::new();
    s += "struct Edge { cell: u32, inner: u32, inlet: u32, pad: u32, vx: f32, vy: f32, vz: f32, pad2: f32 }\n";
    s += "struct Count { n: u32, scale: f32, p1: u32, p2: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> count: Count;\n";
    s += "@group(0) @binding(1) var<storage, read> edges: array<Edge>;\n";
    s += &populations(1, "b");
    if heat {
        s += &thermal_populations(2);
    }
    s += "@compute @workgroup_size(64)\n";
    s += "fn edge(@builtin(global_invocation_id) id: vec3<u32>) {\n";
    s += "  if (id.x >= count.n) { return; }\n";
    s += "  let e = edges[id.x];\n";
    let sum = (0..Q)
        .map(|i| format!("b{i}[e.inner]"))
        .collect::<Vec<_>>()
        .join(" + ");
    s += &format!("  let rn = 1.0 + {sum};\n");
    for (axis, name) in ["x", "y", "z"].iter().enumerate() {
        let terms: Vec<String> = C
            .iter()
            .enumerate()
            .filter(|(_, c)| c[axis] != 0)
            .map(|(i, c)| format!("{}b{i}[e.inner]", if c[axis] > 0 { "+" } else { "-" }))
            .collect();
        s += &format!("  let n{name} = (0.0 {}) / rn;\n", terms.join(" "));
    }
    s += "  var rho = 1.0; var ux = nx; var uy = ny; var uz = nz;\n";
    s += "  if (e.inlet == 1u) { rho = rn; ux = count.scale * e.vx; uy = count.scale * e.vy; uz = count.scale * e.vz; }\n";
    s += "  let uu = ux * ux + uy * uy + uz * uz;\n";
    for (i, c) in C.iter().enumerate() {
        let w = weight(i);
        s += &format!(
            "  {{ let cu = ({}.0) * ux + ({}.0) * uy + ({}.0) * uz; b{i}[e.cell] = {w:.17} * (rho - 1.0 + rho * (3.0 * cu + 4.5 * cu * cu - 1.5 * uu)); }}\n",
            c[0], c[1], c[2]
        );
    }
    if heat {
        for i in 0..TQ {
            s += &format!("  tb{i}[e.cell] = select(tb{i}[e.inner], 0.0, e.inlet == 1u);\n");
        }
    }
    s += "}\n";
    s
}

fn theta_shader(name: &str, add: bool) -> String {
    let mut s = String::new();
    s += "struct Count { n: u32, p1: u32, p2: u32, p3: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> count: Count;\n";
    s += "@group(0) @binding(1) var<storage, read> cells: array<u32>;\n";
    s += "@group(0) @binding(2) var<storage, read_write> out: array<f32>;\n";
    s += &thermal_populations(1);
    s += "@compute @workgroup_size(64)\n";
    s += &format!(
        "fn {name}(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {{\n"
    );
    s += "  let k = id.x + id.y * groups.x * 64u;\n";
    s += "  if (k >= count.n) { return; }\n";
    s += if add {
        "  let cell = k;\n"
    } else {
        "  let cell = cells[k];\n"
    };
    s += "  let theta = ta0[cell] + ta1[cell] + ta2[cell] + ta3[cell] + ta4[cell] + ta5[cell] + ta6[cell];\n";
    s += if add {
        "  out[k] = out[k] + theta;\n"
    } else {
        "  out[k] = theta;\n"
    };
    s += "}\n";
    s
}

fn force_shader() -> String {
    let mut s = String::new();
    s += "struct Params { nx: u32, ny: u32, nz: u32, count: u32 }\n";
    s += "struct Link { cell: u32, body: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> p: Params;\n";
    s += "@group(0) @binding(1) var<storage, read> flags: array<u32>;\n";
    s += "@group(0) @binding(2) var<storage, read> links: array<Link>;\n";
    s += "@group(0) @binding(3) var<storage, read_write> out: array<vec4<f32>>;\n";
    s += "@group(0) @binding(4) var<storage, read> cuts: array<f32>;\n";
    s += "@group(0) @binding(5) var<storage, read> spins: array<vec4<f32>>;\n";
    s += &populations(1, "a");
    s += "fn wrap(v: i32, n: u32) -> u32 { let m = i32(n); return u32(((v % m) + m) % m); }\n";
    s += "@compute @workgroup_size(64)\n";
    s += "fn force(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n";
    s += "  let k = id.x + id.y * groups.x * 64u;\n";
    s += "  if (k >= p.count) { return; }\n";
    s += "  let l = links[k];\n";
    s +=
        "  if ((flags[l.cell] & 255u) != 0u) { out[k] = vec4<f32>(0.0, 0.0, 0.0, 1.0); return; }\n";
    s += "  let x = i32(l.cell % p.nx); let y = i32((l.cell / p.nx) % p.ny); let z = i32(l.cell / (p.nx * p.ny));\n";
    s += "  var f = vec3<f32>(0.0, 0.0, 0.0); var s: u32; var fs: u32; var back: f32;\n";
    s += "  let slot = flags[l.cell] >> 8u;\n";
    for (i, c) in C.iter().enumerate().skip(1) {
        let o = opposite(i);
        s += &format!(
            "  s = wrap(x + ({}), p.nx) + (wrap(y + ({}), p.ny) + wrap(z + ({}), p.nz) * p.ny) * p.nx;\n  fs = flags[s];\n  if (((fs & 255u) == 1u || (fs & 255u) == 2u) && (fs >> 8u) == l.body) {{\n    back = a{i}[l.cell];\n    if (slot > 0u) {{ let q = cuts[(slot - 1u) * 19u + {o}u]; if (q >= 0.0) {{ if (q < 0.5) {{ let n = wrap(x - ({}), p.nx) + (wrap(y - ({}), p.ny) + wrap(z - ({}), p.nz) * p.ny) * p.nx; let kn = flags[n] & 255u; if (kn == 0u || kn == 3u || kn == 4u) {{ back = 2.0 * q * a{i}[l.cell] + (1.0 - 2.0 * q) * a{i}[n]; }} }} else {{ back = (0.5 / q) * a{i}[l.cell] + (1.0 - 0.5 / q) * a{o}[l.cell]; }} }} }}\n    let c = vec3<f32>({}.0, {}.0, {}.0);\n    let u = cross(spins[2u * l.body].xyz, vec3<f32>(f32(x), f32(y), f32(z)) + 0.5 * c - spins[2u * l.body + 1u].xyz);\n    let moved = back - {:.17} * dot(c, u);\n    f = f + (a{i}[l.cell] + moved) * c + (moved - a{i}[l.cell]) * u;\n  }}\n",
            c[0],
            c[1],
            c[2],
            c[0],
            c[1],
            c[2],
            c[0],
            c[1],
            c[2],
            6.0 * weight(i)
        );
    }
    s += &format!(
        "  out[k] = vec4<f32>(f, 1.0{});\n",
        (0..19)
            .map(|i| format!(" + a{i}[l.cell]"))
            .collect::<String>()
    );
    s += "}\n";
    s
}

fn rotor_shader(stage: &str) -> String {
    let mut s = String::new();
    s += "const KIND: u32 = 255u;\n";
    s += "struct Turn { nx: u32, ny: u32, nz: u32, count: u32, body: u32, fx: u32, fy: u32, fz: u32, ox: f32, oy: f32, oz: f32, kx: f32, ky: f32, kz: f32, cos: f32, sin: f32, sx: f32, sy: f32, sz: f32, h: f32, wx: f32, wy: f32, wz: f32, covered: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> p: Turn;\n";
    s += "@group(0) @binding(1) var<storage, read_write> flags: array<u32>;\n";
    s += "@group(0) @binding(2) var<storage, read> cells: array<u32>;\n";
    s += "@group(0) @binding(3) var<storage, read> bits: array<u32>;\n";
    s += "@group(0) @binding(4) var<storage, read_write> fresh: array<u32>;\n";
    s += &populations(1, "b");
    s += "fn solid(q: vec3<f32>) -> bool {\n";
    s += "  let i = round((q - vec3<f32>(p.sx, p.sy, p.sz)) / p.h);\n";
    s += "  if (any(i < vec3<f32>(0.0)) || i.x >= f32(p.fx) || i.y >= f32(p.fy) || i.z >= f32(p.fz)) { return false; }\n";
    s += "  let n = u32(i.x) + (u32(i.y) + u32(i.z) * p.fy) * p.fx;\n";
    s += "  return ((bits[n / 32u] >> (n % 32u)) & 1u) == 1u;\n";
    s += "}\n";
    s += "@compute @workgroup_size(64)\n";
    s += &format!(
        "fn {stage}(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {{\n"
    );
    s += "  let k = id.x + id.y * groups.x * 64u;\n";
    s += "  if (k >= p.count) { return; }\n";
    s += "  let cell = cells[k];\n";
    s += &neighbours("cell");
    s += "  let o = vec3<f32>(p.ox, p.oy, p.oz); let axis = vec3<f32>(p.kx, p.ky, p.kz);\n";
    s += "  let here = vec3<f32>(f32(x), f32(y), f32(z));\n";
    match stage {
        "classify" => {
            s += "  var count = 0u;\n";
            s += "  for (var corner = 0u; corner < 8u; corner = corner + 1u) {\n";
            s += "    let sign = vec3<f32>(f32(corner & 1u), f32((corner >> 1u) & 1u), f32((corner >> 2u) & 1u)) * 2.0 - 1.0;\n";
            s += "    let v = here + 0.5 * p.h * sign - o;\n";
            s += "    let q = o + v * p.cos - cross(axis, v) * p.sin + axis * dot(axis, v) * (1.0 - p.cos);\n";
            s += "    if (solid(q)) { count = count + 1u; }\n";
            s += "  }\n";
            s += "  fresh[k] = select(0u, 2u | (p.body << 8u), count >= p.covered);\n";
        }
        "refill" => {
            s += "  let old = flags[cell];\n";
            s += "  if ((old & KIND) != 2u || fresh[k] != 0u) { return; }\n";
            s += "  var rho = 0.0; var n = 0.0;\n";
            for c in C.iter().skip(1) {
                let sum = (0..Q)
                    .map(|i| format!("b{i}[m]"))
                    .collect::<Vec<_>>()
                    .join(" + ");
                s += &format!(
                    "  {{ let m = {}; if ((flags[m] & KIND) == 0u) {{ rho = rho + 1.0 + {sum}; n = n + 1.0; }} }}\n",
                    offset(c, 1)
                );
            }
            s += "  if (n > 0.0) { rho = rho / n; } else { rho = 1.0; }\n";
            s += "  let u = cross(vec3<f32>(p.wx, p.wy, p.wz), here - o);\n";
            s += "  let uu = dot(u, u);\n";
            for (i, c) in C.iter().enumerate() {
                let w = weight(i);
                s += &format!(
                    "  {{ let cu = dot(vec3<f32>({}.0, {}.0, {}.0), u); b{i}[cell] = {w:.17} * (rho - 1.0 + rho * (3.0 * cu + 4.5 * cu * cu - 1.5 * uu)); }}\n",
                    c[0], c[1], c[2]
                );
            }
        }
        _ => {
            s += "  flags[cell] = fresh[k];\n";
        }
    }
    s += "}\n";
    s
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Turn {
    nx: u32,
    ny: u32,
    nz: u32,
    count: u32,
    body: u32,
    fx: u32,
    fy: u32,
    fz: u32,
    ox: f32,
    oy: f32,
    oz: f32,
    kx: f32,
    ky: f32,
    kz: f32,
    cos: f32,
    sin: f32,
    sx: f32,
    sy: f32,
    sz: f32,
    h: f32,
    wx: f32,
    wy: f32,
    wz: f32,
    covered: u32,
}

const TURN_SLOTS: usize = 200;

struct Turning {
    rotor: Rotor,
    uniform: wgpu::Buffer,
    stride: u64,
    common: wgpu::BindGroup,
    sides: [wgpu::BindGroup; 2],
    count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Edge {
    cell: u32,
    inner: u32,
    inlet: u32,
    pad: u32,
    vx: f32,
    vy: f32,
    vz: f32,
    pad2: f32,
}

struct Pass {
    pipeline: wgpu::ComputePipeline,
    common: wgpu::BindGroup,
    sides: [wgpu::BindGroup; 2],
    count: u32,
    header: wgpu::Buffer,
}

struct Thermal {
    groups: [wgpu::BindGroup; 2],
    copies: [Vec<wgpu::Buffer>; 2],
    mean: (Pass, wgpu::Buffer),
    probes: Option<(Pass, wgpu::Buffer)>,
}

pub struct Lattice<'a> {
    gpu: &'a Gpu,
    pub size: [usize; 3],
    copies: [Vec<wgpu::Buffer>; 2],
    params: wgpu::Buffer,
    thermal: Option<Thermal>,
    pipeline: wgpu::ComputePipeline,
    common: wgpu::BindGroup,
    sides: [[wgpu::BindGroup; 2]; 2],
    edges: Option<Pass>,
    forces: Option<(Pass, wgpu::Buffer)>,
    planes: (Pass, wgpu::Buffer),
    mean: (Pass, wgpu::Buffer, usize, [usize; 3]),
    now: (Pass, wgpu::Buffer),
    parity: usize,
    last: Option<wgpu::SubmissionIndex>,
    half: bool,
    pub links: Vec<[u32; 2]>,
    turning: Vec<Turning>,
    turners: Option<[wgpu::ComputePipeline; 3]>,
    turned: u64,
}

pub struct MeanField {
    pub stride: usize,
    pub size: [usize; 3],
    pub velocity: Vec<Option<[f32; 3]>>,
    pub density: Vec<Option<f32>>,
}

pub struct Fields {
    pub density: Vec<f64>,
    pub velocity: Vec<[f64; 3]>,
}

fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let fraction = (bits & 0x3ff) as f32;
    sign * match exponent {
        0 => fraction * 2f32.powi(-24),
        31 if fraction == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        e => (1.0 + fraction / 1024.0) * 2f32.powi(e - 15),
    }
}

fn typed(source: &str, half: bool) -> String {
    let mut out = String::from(if half {
        "enable f16;\nalias P = f16;\nconst SCALE: f32 = 32768.0;\n"
    } else {
        "alias P = f32;\nconst SCALE: f32 = 1.0;\n"
    });
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut wrap_until_semicolon = false;
    while i < bytes.len() {
        let starts = (bytes[i] == b'a' || bytes[i] == b'b')
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'));
        let digits = bytes[i + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if starts && digits > 0 && bytes.get(i + 1 + digits) == Some(&b'[') {
            let close = i + 1 + digits + source[i + 1 + digits..].find(']').unwrap_or(0);
            let access = &source[i..=close];
            if source[close + 1..].starts_with(" = ") {
                out.push_str(access);
                out.push_str(" = P(SCALE * (");
                i = close + 4;
                wrap_until_semicolon = true;
            } else {
                out.push_str("(f32(");
                out.push_str(access);
                out.push_str(") / SCALE)");
                i = close + 1;
            }
            continue;
        }
        if wrap_until_semicolon && bytes[i] == b';' {
            out.push_str("))");
            wrap_until_semicolon = false;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn dispatch(count: u32) -> (u32, u32) {
    let groups = count.div_ceil(64).max(1);
    if groups <= 65535 {
        (groups, 1)
    } else {
        (65535, groups.div_ceil(65535))
    }
}

impl<'a> Lattice<'a> {
    pub fn new(gpu: &'a Gpu, domain: &Domain) -> Result<Lattice<'a>> {
        let invalid = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let short = gpu.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let lattice = Lattice::build(gpu, domain);
        let cells = domain.size.iter().product::<usize>();
        if let Some(error) = pollster::block_on(short.pop()) {
            bail!(
                "the GPU has no room for {:.1} million cells ({error})",
                cells as f64 / 1.0e6
            );
        }
        if let Some(error) = pollster::block_on(invalid.pop()) {
            bail!("the GPU refused the lattice: {error}");
        }
        lattice
    }

    fn build(gpu: &'a Gpu, domain: &Domain) -> Result<Lattice<'a>> {
        let size = domain.size;
        let count = size[0] * size[1] * size[2];
        if domain.flags.len() != count || domain.body.len() != count {
            bail!(
                "{} flags and {} bodies for {count} cells",
                domain.flags.len(),
                domain.body.len()
            );
        }
        let half = domain.half;
        if half && !gpu.device.features().contains(wgpu::Features::SHADER_F16) {
            bail!("{} has no 16-bit floats for `precision half`", gpu.name);
        }
        let bytes = (count as u64 * if half { 2 } else { 4 }).next_multiple_of(4);
        if bytes > gpu.max_binding() {
            bail!(
                "{count} cells need {} MB per direction, over the device's {} MB per buffer",
                bytes >> 20,
                gpu.max_binding() >> 20
            );
        }
        let device = &gpu.device;
        let make = |label: &str| -> Vec<wgpu::Buffer> {
            (0..Q)
                .map(|_| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(label),
                        size: bytes,
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                })
                .collect()
        };
        let copies = [make("lattice a"), make("lattice b")];
        let mut words: Vec<u32> = domain
            .flags
            .iter()
            .zip(&domain.body)
            .map(|(&f, &b)| f as u32 | (b as u32) << 8)
            .collect();
        let mut table: Vec<f32> = Vec::new();
        let mut slots: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        let swept: std::collections::HashSet<usize> = domain
            .rotors
            .iter()
            .flat_map(|r| r.cells.iter().copied())
            .collect();
        for &(cell, direction, q) in &domain.cuts {
            if domain.flags[cell] != FLUID
                || direction >= Q
                || !(0.0..=1.0).contains(&q)
                || swept.contains(&cell)
            {
                continue;
            }
            let slot = *slots.entry(cell).or_insert_with(|| {
                table.extend(std::iter::repeat_n(-1.0, Q));
                (table.len() / Q) as u32
            });
            table[(slot as usize - 1) * Q + direction] = q as f32;
            words[cell] = FLUID as u32 | slot << 8;
        }
        if table.is_empty() {
            table.extend(std::iter::repeat_n(-1.0, Q));
        }
        let cuts = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cuts"),
            contents: bytemuck::cast_slice(&table),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let flags = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("flags"),
            contents: bytemuck::cast_slice(&words),
            usage: wgpu::BufferUsages::STORAGE,
        });
        if domain.heat.is_some() && !domain.rotors.is_empty() {
            bail!("a turning rotor cannot carry heat yet");
        }
        let bodies = domain
            .body
            .iter()
            .copied()
            .chain(domain.rotors.iter().map(|r| r.body))
            .max()
            .unwrap_or(0) as usize
            + 1;
        let mut spin_table = vec![[0.0f32; 4]; 2 * bodies];
        for rotor in &domain.rotors {
            let b = rotor.body as usize;
            spin_table[2 * b] = [
                (rotor.axis[0] * rotor.omega) as f32,
                (rotor.axis[1] * rotor.omega) as f32,
                (rotor.axis[2] * rotor.omega) as f32,
                0.0,
            ];
            spin_table[2 * b + 1] = [
                rotor.centre[0] as f32,
                rotor.centre[1] as f32,
                rotor.centre[2] as f32,
                0.0,
            ];
        }
        let spins = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("spins"),
            contents: bytemuck::cast_slice(&spin_table),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entry = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let read = wgpu::BufferBindingType::Storage { read_only: true };
        let write = wgpu::BufferBindingType::Storage { read_only: false };
        let uniform = wgpu::BufferBindingType::Uniform;
        let layout = |entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries,
            })
        };
        let group = |layout: &wgpu::BindGroupLayout, buffers: &[&wgpu::Buffer]| {
            let entries: Vec<wgpu::BindGroupEntry> = buffers
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            })
        };
        let numbered = |layout: &wgpu::BindGroupLayout, buffers: &[(u32, &wgpu::Buffer)]| {
            let entries: Vec<wgpu::BindGroupEntry> = buffers
                .iter()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: *i,
                    resource: b.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            })
        };
        let pipeline = |source: String, entry_point: &str, layouts: &[&wgpu::BindGroupLayout]| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(entry_point),
                source: wgpu::ShaderSource::Wgsl(typed(&source, half).into()),
            });
            let groups: Vec<Option<&wgpu::BindGroupLayout>> =
                layouts.iter().map(|l| Some(*l)).collect();
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(entry_point),
                bind_group_layouts: &groups,
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let side_entries: Vec<wgpu::BindGroupLayoutEntry> =
            (0..Q as u32).map(|i| entry(i, write)).collect();
        let side_layout = layout(&side_entries);
        let side = |which: usize| -> wgpu::BindGroup {
            let refs: Vec<&wgpu::Buffer> = copies[which].iter().collect();
            group(&side_layout, &refs)
        };
        let heat = domain.heat.as_ref().map(|h| -> Result<_> {
            if h.cells.len() != count {
                bail!("{} heat cells for {count} cells", h.cells.len());
            }
            let needed = 3 * Q as u32 + 4;
            if gpu.device.limits().max_storage_buffers_per_shader_stage < needed {
                bail!(
                    "{} cannot bind the {needed} buffers a heated flow needs",
                    gpu.name
                );
            }
            let cells = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("heat"),
                contents: bytemuck::cast_slice(&h.cells),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let make = || -> Vec<wgpu::Buffer> {
                (0..TQ)
                    .map(|_| {
                        device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("temperature"),
                            size: count as u64 * 4,
                            usage: wgpu::BufferUsages::STORAGE
                                | wgpu::BufferUsages::COPY_SRC
                                | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        })
                    })
                    .collect()
            };
            Ok((cells, [make(), make()]))
        });
        let heat = heat.transpose()?;
        let thermal_layout = layout(
            &(0..2 * TQ as u32)
                .map(|i| entry(i, write))
                .collect::<Vec<_>>(),
        );
        let thermal_group = |copies: &[Vec<wgpu::Buffer>; 2], parity: usize| {
            let refs: Vec<&wgpu::Buffer> = copies[parity]
                .iter()
                .chain(copies[1 - parity].iter())
                .collect();
            group(&thermal_layout, &refs)
        };
        let thermal_groups = heat
            .as_ref()
            .map(|(_, copies)| [thermal_group(copies, 0), thermal_group(copies, 1)]);
        let (common_layout, step, common) = match &heat {
            None => {
                let common_layout = layout(&[
                    entry(0, uniform),
                    entry(1, read),
                    entry(2, read),
                    entry(4, read),
                ]);
                let step = pipeline(
                    shader(false),
                    "step",
                    &[&common_layout, &side_layout, &side_layout],
                );
                let common = numbered(
                    &common_layout,
                    &[(0, &params), (1, &flags), (2, &cuts), (4, &spins)],
                );
                (common_layout, step, common)
            }
            Some((cells, _)) => {
                let common_layout = layout(&[
                    entry(0, uniform),
                    entry(1, read),
                    entry(2, read),
                    entry(3, read),
                    entry(4, read),
                ]);
                let step = pipeline(
                    shader(true),
                    "step",
                    &[&common_layout, &side_layout, &side_layout, &thermal_layout],
                );
                let common = group(&common_layout, &[&params, &flags, &cuts, cells, &spins]);
                (common_layout, step, common)
            }
        };
        drop(common_layout);
        let sides = [[side(0), side(1)], [side(1), side(0)]];
        let mut edges: Vec<Edge> = domain
            .inflow
            .iter()
            .map(|&(cell, inner, v)| Edge {
                cell: cell as u32,
                inner: inner as u32,
                inlet: 1,
                pad: 0,
                vx: v[0] as f32,
                vy: v[1] as f32,
                vz: v[2] as f32,
                pad2: 0.0,
            })
            .collect();
        edges.extend(domain.outflow.iter().map(|&(cell, inner)| Edge {
            cell: cell as u32,
            inner: inner as u32,
            inlet: 0,
            pad: 0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            pad2: 0.0,
        }));
        let edges = (!edges.is_empty()).then(|| {
            let list = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("edges"),
                contents: bytemuck::cast_slice(&edges),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let n = edges.len() as u32;
            let counter = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("edge count"),
                contents: bytemuck::cast_slice(&[n, 1.0f32.to_bits(), 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let edge_layout = layout(&[entry(0, uniform), entry(1, read)]);
            Pass {
                pipeline: match heat {
                    None => pipeline(edge_shader(false), "edge", &[&edge_layout, &side_layout]),
                    Some(_) => pipeline(
                        edge_shader(true),
                        "edge",
                        &[&edge_layout, &side_layout, &thermal_layout],
                    ),
                },
                common: group(&edge_layout, &[&counter, &list]),
                sides: [side(1), side(0)],
                count: n,
                header: counter,
            }
        });
        let rotor_cells: Vec<std::collections::HashSet<usize>> = domain
            .rotors
            .iter()
            .map(|r| r.cells.iter().copied().collect())
            .collect();
        let swept_by = |r: &Rotor, cell: usize| {
            domain
                .rotors
                .iter()
                .position(|o| o.body == r.body)
                .is_some_and(|k| rotor_cells[k].contains(&cell))
        };
        let mut links: Vec<[u32; 2]> = Vec::new();
        for cell in 0..count {
            if domain.flags[cell] != FLUID && !swept.contains(&cell) {
                continue;
            }
            let (x, y, z) = (
                (cell % size[0]) as i64,
                ((cell / size[0]) % size[1]) as i64,
                (cell / (size[0] * size[1])) as i64,
            );
            let mut bodies: Vec<u16> = C
                .iter()
                .skip(1)
                .filter_map(|c| {
                    let w = |v: i64, n: usize| v.rem_euclid(n as i64) as usize;
                    let s = w(x + c[0] as i64, size[0])
                        + (w(y + c[1] as i64, size[1]) + w(z + c[2] as i64, size[2]) * size[1])
                            * size[0];
                    (matches!(domain.flags[s], WALL | MOVING)
                        && domain.body[s] > 0
                        && !domain.rotors.iter().any(|r| r.body == domain.body[s]))
                    .then_some(domain.body[s])
                })
                .collect();
            bodies.extend(
                domain
                    .rotors
                    .iter()
                    .filter(|r| swept_by(r, cell))
                    .map(|r| r.body),
            );
            bodies.sort_unstable();
            bodies.dedup();
            links.extend(bodies.into_iter().map(|b| [cell as u32, b as u32]));
        }
        let forces = (!links.is_empty()).then(|| {
            let n = links.len() as u32;
            let list = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("links"),
                contents: bytemuck::cast_slice(&links),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let out = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("link forces"),
                size: n as u64 * 16,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("force params"),
                contents: bytemuck::cast_slice(&[
                    size[0] as u32,
                    size[1] as u32,
                    size[2] as u32,
                    n,
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let force_layout = layout(&[
                entry(0, uniform),
                entry(1, read),
                entry(2, read),
                entry(3, write),
                entry(4, read),
                entry(5, read),
            ]);
            let pass = Pass {
                pipeline: pipeline(force_shader(), "force", &[&force_layout, &side_layout]),
                common: group(
                    &force_layout,
                    &[&header, &flags, &list, &out, &cuts, &spins],
                ),
                sides: [side(0), side(1)],
                count: n,
                header: header.clone(),
            };
            (pass, out)
        });
        let widest = (size[0] * size[1])
            .max(size[1] * size[2])
            .max(size[0] * size[2]);
        let plane_out = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plane"),
            size: widest as u64 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let plane_header = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plane params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let plane_layout = layout(&[entry(0, uniform), entry(1, read), entry(2, write)]);
        let planes = (
            Pass {
                pipeline: pipeline(plane_shader(), "plane", &[&plane_layout, &side_layout]),
                common: group(&plane_layout, &[&plane_header, &flags, &plane_out]),
                sides: [side(0), side(1)],
                count: widest as u32,
                header: plane_header,
            },
            plane_out,
        );
        let stride = mean_stride(size);
        let coarse = size.map(|n| n.div_ceil(stride));
        let coarse_count = coarse.iter().product::<usize>();
        let mean_out = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mean"),
            size: coarse_count as u64 * 32,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mean_header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mean params"),
            contents: bytemuck::cast_slice(&[
                size[0] as u32,
                size[1] as u32,
                size[2] as u32,
                stride as u32,
                coarse[0] as u32,
                coarse[1] as u32,
                coarse[2] as u32,
                coarse_count as u32,
            ]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let mean = (
            Pass {
                pipeline: pipeline(mean_shader(true), "mean", &[&plane_layout, &side_layout]),
                common: group(&plane_layout, &[&mean_header, &flags, &mean_out]),
                sides: [side(0), side(1)],
                count: coarse_count as u32,
                header: mean_header,
            },
            mean_out,
            stride,
            coarse,
        );
        let now_out = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snapshot"),
            size: coarse_count as u64 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let now = (
            Pass {
                pipeline: pipeline(mean_shader(false), "mean", &[&plane_layout, &side_layout]),
                common: group(&plane_layout, &[&mean.0.header, &flags, &now_out]),
                sides: [side(0), side(1)],
                count: coarse_count as u32,
                header: mean.0.header.clone(),
            },
            now_out,
        );
        let thermal = match (heat, thermal_groups) {
            (Some((_, copies)), Some(groups)) => {
                let theta_layout = layout(&[entry(0, uniform), entry(1, read), entry(2, write)]);
                let theta_pass = |name: &str, add: bool, n: usize, cells: &[u32]| {
                    let list = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("theta cells"),
                        contents: bytemuck::cast_slice(if cells.is_empty() {
                            &[0u32]
                        } else {
                            cells
                        }),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                    let out = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(name),
                        size: n.max(1) as u64 * 4,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    });
                    let header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("theta count"),
                        contents: bytemuck::cast_slice(&[n as u32, 0, 0, 0]),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    (
                        Pass {
                            pipeline: pipeline(
                                theta_shader(name, add),
                                name,
                                &[&theta_layout, &thermal_layout],
                            ),
                            common: group(&theta_layout, &[&header, &list, &out]),
                            sides: [thermal_group(&copies, 0), thermal_group(&copies, 1)],
                            count: n as u32,
                            header,
                        },
                        out,
                    )
                };
                let probes: Vec<u32> = domain
                    .heat
                    .as_ref()
                    .map(|h| h.probes.iter().map(|&c| c as u32).collect())
                    .unwrap_or_default();
                Some(Thermal {
                    mean: theta_pass("tmean", true, count, &[]),
                    probes: (!probes.is_empty())
                        .then(|| theta_pass("tprobe", false, probes.len(), &probes)),
                    groups,
                    copies,
                })
            }
            _ => None,
        };
        let alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let stride = (std::mem::size_of::<Turn>() as u64).next_multiple_of(alignment);
        let turn_layout = layout(&[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Turn>() as u64),
                },
                count: None,
            },
            entry(1, write),
            entry(2, read),
            entry(3, read),
            entry(4, write),
        ]);
        let turners = (!domain.rotors.is_empty()).then(|| {
            ["classify", "refill", "commit"]
                .map(|stage| pipeline(rotor_shader(stage), stage, &[&turn_layout, &side_layout]))
        });
        let turning: Vec<Turning> = domain
            .rotors
            .iter()
            .map(|rotor| {
                let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("turn"),
                    size: stride * TURN_SLOTS as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let cells: Vec<u32> = rotor.cells.iter().map(|&c| c as u32).collect();
                let list = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("rotor cells"),
                    contents: bytemuck::cast_slice(if cells.is_empty() { &[0u32] } else { &cells }),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let bits = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("rotor bits"),
                    contents: bytemuck::cast_slice(&rotor.bits()),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let fresh = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("rotor fresh"),
                    size: cells.len().max(1) as u64 * 4,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                });
                let common = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &turn_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &uniform_buffer,
                                offset: 0,
                                size: wgpu::BufferSize::new(std::mem::size_of::<Turn>() as u64),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: flags.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: list.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: bits.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: fresh.as_entire_binding(),
                        },
                    ],
                });
                Turning {
                    rotor: rotor.clone(),
                    uniform: uniform_buffer,
                    stride,
                    common,
                    sides: [side(1), side(0)],
                    count: cells.len() as u32,
                }
            })
            .collect();
        Ok(Lattice {
            gpu,
            size,
            copies,
            params,
            thermal,
            pipeline: step,
            common,
            sides,
            edges,
            forces,
            planes,
            mean,
            now,
            parity: 0,
            last: None,
            half,
            links,
            turning,
            turners,
            turned: 0,
        })
    }

    pub fn run(&mut self, settings: &Settings, steps: usize) -> Result<()> {
        let count = self.size.iter().product::<usize>() as u32;
        let params = Params {
            nx: self.size[0] as u32,
            ny: self.size[1] as u32,
            nz: self.size[2] as u32,
            count,
            omega: (1.0 / (3.0 * settings.viscosity + 0.5)) as f32,
            fx: settings.force[0] as f32,
            fy: settings.force[1] as f32,
            fz: settings.force[2] as f32,
            wx: settings.wall[0] as f32,
            wy: settings.wall[1] as f32,
            wz: settings.wall[2] as f32,
            regular: if settings.collision == Collision::Regularized {
                1.0
            } else {
                0.0
            },
            smagorinsky: settings.smagorinsky as f32,
            sponge: settings.sponge as f32,
            bulk: settings.bulk as f32,
            sx: settings.stream[0] as f32,
            sy: settings.stream[1] as f32,
            sz: settings.stream[2] as f32,
            axis: settings.sponge_axis as u32,
        };
        self.gpu
            .queue
            .write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        if let Some(edges) = &self.edges {
            self.gpu.queue.write_buffer(
                &edges.header,
                4,
                bytemuck::bytes_of(&(settings.inflow as f32)),
            );
        }
        let (gx, gy) = dispatch(count);
        let mut left = steps;
        while left > 0 {
            let batch = left.min(200).min(TURN_SLOTS);
            for turning in &self.turning {
                let r = &turning.rotor;
                let mut bytes = vec![0u8; turning.stride as usize * batch];
                for j in 0..batch {
                    let angle = r.omega * (self.turned + j as u64 + 1) as f64;
                    let angle = angle.rem_euclid(std::f64::consts::TAU);
                    let turn = Turn {
                        nx: self.size[0] as u32,
                        ny: self.size[1] as u32,
                        nz: self.size[2] as u32,
                        count: turning.count,
                        body: r.body as u32,
                        fx: r.size[0] as u32,
                        fy: r.size[1] as u32,
                        fz: r.size[2] as u32,
                        ox: r.centre[0] as f32,
                        oy: r.centre[1] as f32,
                        oz: r.centre[2] as f32,
                        kx: r.axis[0] as f32,
                        ky: r.axis[1] as f32,
                        kz: r.axis[2] as f32,
                        cos: angle.cos() as f32,
                        sin: angle.sin() as f32,
                        sx: r.start[0] as f32,
                        sy: r.start[1] as f32,
                        sz: r.start[2] as f32,
                        h: (1.0 / SUBCELLS as f64) as f32,
                        wx: (r.axis[0] * r.omega) as f32,
                        wy: (r.axis[1] * r.omega) as f32,
                        wz: (r.axis[2] * r.omega) as f32,
                        covered: COVERED,
                    };
                    let at = j * turning.stride as usize;
                    bytes[at..at + std::mem::size_of::<Turn>()]
                        .copy_from_slice(bytemuck::bytes_of(&turn));
                }
                self.gpu.queue.write_buffer(&turning.uniform, 0, &bytes);
            }
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            for step in 0..batch {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.common, &[]);
                pass.set_bind_group(1, &self.sides[self.parity][0], &[]);
                pass.set_bind_group(2, &self.sides[self.parity][1], &[]);
                if let Some(thermal) = &self.thermal {
                    pass.set_bind_group(3, &thermal.groups[self.parity], &[]);
                }
                pass.dispatch_workgroups(gx, gy, 1);
                if let Some(edges) = &self.edges {
                    pass.set_pipeline(&edges.pipeline);
                    pass.set_bind_group(0, &edges.common, &[]);
                    pass.set_bind_group(1, &edges.sides[self.parity], &[]);
                    if let Some(thermal) = &self.thermal {
                        pass.set_bind_group(2, &thermal.groups[self.parity], &[]);
                    }
                    pass.dispatch_workgroups(edges.count.div_ceil(64), 1, 1);
                }
                if let Some(turners) = &self.turners {
                    for turning in &self.turning {
                        let offset = (step * turning.stride as usize) as u32;
                        let (tx, ty) = dispatch(turning.count);
                        for stage in turners {
                            pass.set_pipeline(stage);
                            pass.set_bind_group(0, &turning.common, &[offset]);
                            pass.set_bind_group(1, &turning.sides[self.parity], &[]);
                            pass.dispatch_workgroups(tx, ty, 1);
                        }
                    }
                }
                drop(pass);
                self.parity ^= 1;
            }
            self.turned += batch as u64;
            let index = self.gpu.queue.submit([encoder.finish()]);
            if let Some(previous) = self.last.replace(index) {
                self.gpu.wait(previous)?;
            }
            left -= batch;
        }
        Ok(())
    }

    pub fn finish(&self) -> Result<()> {
        self.gpu.finish()
    }

    fn encode(&self, pass: &Pass, count: u32) -> wgpu::CommandEncoder {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&pass.pipeline);
            compute.set_bind_group(0, &pass.common, &[]);
            compute.set_bind_group(1, &pass.sides[self.parity], &[]);
            let (gx, gy) = dispatch(count);
            compute.dispatch_workgroups(gx, gy, 1);
        }
        encoder
    }

    pub fn forces(&self, bodies: usize) -> Result<Vec<[f64; 3]>> {
        let mut out = vec![[0.0; 3]; bodies];
        let values = self.link_samples()?;
        for (k, link) in self.links.iter().enumerate() {
            if let Some(total) = out.get_mut(link[1] as usize - 1) {
                for d in 0..3 {
                    total[d] += values[k][d] as f64;
                }
            }
        }
        Ok(out)
    }

    pub fn plane(&self, axis: usize, index: usize) -> Result<(usize, Vec<[f32; 4]>)> {
        self.take_plane(self.request_plane(axis, index))
    }

    pub fn request_plane(&self, axis: usize, index: usize) -> PendingPlane {
        let others: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
        let (width, height) = (self.size[others[0]], self.size[others[1]]);
        let count = width * height;
        let (pass, buffer) = &self.planes;
        let header = [
            self.size[0] as u32,
            self.size[1] as u32,
            self.size[2] as u32,
            axis as u32,
            index as u32,
            width as u32,
            count as u32,
            0,
        ];
        self.gpu
            .queue
            .write_buffer(&pass.header, 0, bytemuck::cast_slice(&header));
        let encoder = self.encode(pass, count as u32);
        PendingPlane {
            width,
            count,
            reading: self.gpu.fetch(encoder, buffer),
        }
    }

    pub fn take_plane(&self, pending: PendingPlane) -> Result<(usize, Vec<[f32; 4]>)> {
        let values = self.gpu.take(pending.reading)?;
        Ok((
            pending.width,
            values
                .chunks_exact(4)
                .take(pending.count)
                .map(|v| [v[0], v[1], v[2], v[3]])
                .collect(),
        ))
    }

    pub fn accumulate(&self) {
        let (pass, ..) = &self.mean;
        let encoder = self.encode(pass, pass.count);
        self.gpu.queue.submit([encoder.finish()]);
    }

    pub fn accumulate_temperature(&self) {
        if let Some(thermal) = &self.thermal {
            let encoder = self.encode(&thermal.mean.0, thermal.mean.0.count);
            self.gpu.queue.submit([encoder.finish()]);
        }
    }

    pub fn temperature_sums(&self) -> Result<Vec<f32>> {
        match &self.thermal {
            Some(thermal) => self.gpu.read(&thermal.mean.1),
            None => Ok(Vec::new()),
        }
    }

    pub fn temperatures(&self) -> Result<Vec<f32>> {
        let Some(thermal) = &self.thermal else {
            return Ok(Vec::new());
        };
        let mut total = vec![0.0f32; self.size.iter().product()];
        for buffer in &thermal.copies[self.parity] {
            for (t, v) in total.iter_mut().zip(self.gpu.read(buffer)?) {
                *t += v;
            }
        }
        Ok(total)
    }

    pub fn request_probes(&self) -> PendingProbes {
        PendingProbes(self.thermal.as_ref().and_then(|t| t.probes.as_ref()).map(
            |(pass, buffer)| {
                let encoder = self.encode(pass, pass.count);
                self.gpu.fetch(encoder, buffer)
            },
        ))
    }

    pub fn take_probes(&self, pending: PendingProbes) -> Result<Vec<f32>> {
        match pending.0 {
            Some(reading) => self.gpu.take(reading),
            None => Ok(Vec::new()),
        }
    }

    pub fn snapshot(&self) -> Result<MeanField> {
        self.take_snapshot(self.request_snapshot())
    }

    pub fn request_snapshot(&self) -> PendingSnapshot {
        let (pass, buffer) = &self.now;
        let encoder = self.encode(pass, pass.count);
        PendingSnapshot(self.gpu.fetch(encoder, buffer))
    }

    pub fn take_snapshot(&self, pending: PendingSnapshot) -> Result<MeanField> {
        let (_, _, stride, size) = &self.mean;
        Ok(field(self.gpu.take(pending.0)?, *stride, *size, false))
    }

    pub fn mean_field(&self) -> Result<MeanField> {
        let (_, buffer, stride, size) = &self.mean;
        Ok(field(self.gpu.read(buffer)?, *stride, *size, true))
    }
}

fn field(values: Vec<f32>, stride: usize, size: [usize; 3], with_density: bool) -> MeanField {
    let width = if with_density { 8 } else { 4 };
    let mut velocity = Vec::with_capacity(values.len() / width);
    let mut density = Vec::new();
    for v in values.chunks_exact(width) {
        let fluid = v[3] > 0.0;
        velocity.push(fluid.then(|| [v[0] / v[3], v[1] / v[3], v[2] / v[3]]));
        if with_density {
            density.push(fluid.then(|| v[4] / v[3]));
        }
    }
    MeanField {
        stride,
        size,
        velocity,
        density,
    }
}

pub struct PendingPlane {
    width: usize,
    count: usize,
    reading: Reading,
}

pub struct PendingSnapshot(Reading);

pub struct PendingLinks(Option<Reading>);

pub struct PendingProbes(Option<Reading>);

impl<'a> Lattice<'a> {
    pub fn link_samples(&self) -> Result<Vec<[f32; 4]>> {
        self.take_links(self.request_links())
    }

    pub fn request_links(&self) -> PendingLinks {
        PendingLinks(self.forces.as_ref().map(|(pass, buffer)| {
            let encoder = self.encode(pass, pass.count);
            self.gpu.fetch(encoder, buffer)
        }))
    }

    pub fn take_links(&self, pending: PendingLinks) -> Result<Vec<[f32; 4]>> {
        let Some(reading) = pending.0 else {
            return Ok(Vec::new());
        };
        Ok(self
            .gpu
            .take(reading)?
            .chunks_exact(4)
            .map(|v| [v[0], v[1], v[2], v[3]])
            .collect())
    }

    pub fn fields(&self, force: [f64; 3]) -> Result<Fields> {
        let current = &self.copies[self.parity];
        let populations: Vec<Vec<f32>> = current
            .iter()
            .map(|b| {
                let words = self.gpu.read(b)?;
                Ok(match self.half {
                    false => words,
                    true => words
                        .iter()
                        .flat_map(|w| {
                            let bits = w.to_bits();
                            [bits as u16, (bits >> 16) as u16].map(|h| half_to_f32(h) / 32768.0)
                        })
                        .collect(),
                })
            })
            .collect::<Result<_>>()?;
        let count = self.size.iter().product::<usize>();
        let mut density = vec![0.0; count];
        let mut velocity = vec![[0.0; 3]; count];
        for cell in 0..count {
            let mut rho = 1.0;
            let mut m = [0.0f64; 3];
            for (i, c) in C.iter().enumerate() {
                let g = populations[i][cell] as f64;
                rho += g;
                for k in 0..3 {
                    m[k] += c[k] as f64 * g;
                }
            }
            density[cell] = rho;
            velocity[cell] = [0, 1, 2].map(|k| (m[k] + 0.5 * force[k]) / rho);
        }
        Ok(Fields { density, velocity })
    }
}
