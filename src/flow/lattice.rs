use super::gpu::Gpu;
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

fn shader() -> String {
    let mut s = String::new();
    s += "struct Params { nx: u32, ny: u32, nz: u32, count: u32, omega: f32, fx: f32, fy: f32, fz: f32, wx: f32, wy: f32, wz: f32, regular: f32, smagorinsky: f32, sponge: f32, bulk: f32, sx: f32, sy: f32, sz: f32, axis: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> p: Params;\n";
    s += "@group(0) @binding(1) var<storage, read> flags: array<u32>;\n";
    s += "@group(0) @binding(2) var<storage, read> cuts: array<f32>;\n";
    for i in 0..Q {
        s += &format!("@group(1) @binding({i}) var<storage, read_write> a{i}: array<f32>;\n");
        s += &format!("@group(2) @binding({i}) var<storage, read_write> b{i}: array<f32>;\n");
    }
    s += "fn wrap(v: i32, n: u32) -> u32 { let m = i32(n); return u32(((v % m) + m) % m); }\n";
    s += "@compute @workgroup_size(64)\n";
    s += "fn step(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n";
    s += "  let index = id.x + id.y * groups.x * 64u;\n";
    s += "  if (index >= p.count) { return; }\n";
    s += "  let own = flags[index];\n";
    s += "  if ((own & 255u) != 0u) { return; }\n";
    s += "  let slot = own >> 8u;\n";
    s += "  let x = i32(index % p.nx); let y = i32((index / p.nx) % p.ny); let z = i32(index / (p.nx * p.ny));\n";
    s += "  var s: u32; var fs: u32;\n";
    for (i, c) in C.iter().enumerate() {
        let w = weight(i);
        let o = opposite(i);
        s += &format!(
            "  s = wrap(x - ({}), p.nx) + (wrap(y - ({}), p.ny) + wrap(z - ({}), p.nz) * p.ny) * p.nx;\n",
            c[0], c[1], c[2]
        );
        s += "  fs = flags[s] & 255u;\n";
        s += &format!(
            "  var g{i}: f32 = a{i}[s];\n  if (fs == 1u || fs == 2u) {{ g{i} = a{o}[index]; if (slot > 0u) {{ let q = cuts[(slot - 1u) * 19u + {i}u]; if (q >= 0.0) {{ if (q < 0.5) {{ let n = wrap(x + ({cx}), p.nx) + (wrap(y + ({cy}), p.ny) + wrap(z + ({cz}), p.nz) * p.ny) * p.nx; let kn = flags[n] & 255u; if (kn == 0u || kn == 3u || kn == 4u) {{ g{i} = 2.0 * q * a{o}[index] + (1.0 - 2.0 * q) * a{o}[n]; }} }} else {{ g{i} = (0.5 / q) * a{o}[index] + (1.0 - 0.5 / q) * a{i}[index]; }} }} }} if (fs == 2u) {{ g{i} = g{i} + {:.17} * (({}.0) * p.wx + ({}.0) * p.wy + ({}.0) * p.wz); }} }}\n",
            6.0 * w,
            c[0],
            c[1],
            c[2],
            cx = c[0],
            cy = c[1],
            cz = c[2]
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
    s += "}\n";
    s
}

fn populations(group: u32, name: &str) -> String {
    (0..Q)
        .map(|i| {
            format!(
                "@group({group}) @binding({i}) var<storage, read_write> {name}{i}: array<f32>;\n"
            )
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

fn mean_shader() -> String {
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
    s += "  } } }\n";
    s += "  out[k] = out[k] + total;\n";
    s += "}\n";
    s
}

fn edge_shader() -> String {
    let mut s = String::new();
    s += "struct Edge { cell: u32, inner: u32, inlet: u32, pad: u32, vx: f32, vy: f32, vz: f32, pad2: f32 }\n";
    s += "struct Count { n: u32, scale: f32, p1: u32, p2: u32 }\n";
    s += "@group(0) @binding(0) var<uniform> count: Count;\n";
    s += "@group(0) @binding(1) var<storage, read> edges: array<Edge>;\n";
    s += &populations(1, "b");
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
    s += &populations(1, "a");
    s += "fn wrap(v: i32, n: u32) -> u32 { let m = i32(n); return u32(((v % m) + m) % m); }\n";
    s += "@compute @workgroup_size(64)\n";
    s += "fn force(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n";
    s += "  let k = id.x + id.y * groups.x * 64u;\n";
    s += "  if (k >= p.count) { return; }\n";
    s += "  let l = links[k];\n";
    s += "  let x = i32(l.cell % p.nx); let y = i32((l.cell / p.nx) % p.ny); let z = i32(l.cell / (p.nx * p.ny));\n";
    s += "  var f = vec3<f32>(0.0, 0.0, 0.0); var s: u32; var fs: u32; var back: f32;\n";
    s += "  let slot = flags[l.cell] >> 8u;\n";
    for (i, c) in C.iter().enumerate().skip(1) {
        let o = opposite(i);
        s += &format!(
            "  s = wrap(x + ({}), p.nx) + (wrap(y + ({}), p.ny) + wrap(z + ({}), p.nz) * p.ny) * p.nx;\n  fs = flags[s];\n  if (((fs & 255u) == 1u || (fs & 255u) == 2u) && (fs >> 8u) == l.body) {{\n    back = a{i}[l.cell];\n    if (slot > 0u) {{ let q = cuts[(slot - 1u) * 19u + {o}u]; if (q >= 0.0) {{ if (q < 0.5) {{ let n = wrap(x - ({}), p.nx) + (wrap(y - ({}), p.ny) + wrap(z - ({}), p.nz) * p.ny) * p.nx; let kn = flags[n] & 255u; if (kn == 0u || kn == 3u || kn == 4u) {{ back = 2.0 * q * a{i}[l.cell] + (1.0 - 2.0 * q) * a{i}[n]; }} }} else {{ back = (0.5 / q) * a{i}[l.cell] + (1.0 - 0.5 / q) * a{o}[l.cell]; }} }} }}\n    f = f + (a{i}[l.cell] + back) * vec3<f32>({}.0, {}.0, {}.0);\n  }}\n",
            c[0], c[1], c[2], c[0], c[1], c[2], c[0], c[1], c[2]
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

pub struct Lattice<'a> {
    gpu: &'a Gpu,
    pub size: [usize; 3],
    copies: [Vec<wgpu::Buffer>; 2],
    params: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    common: wgpu::BindGroup,
    sides: [[wgpu::BindGroup; 2]; 2],
    edges: Option<Pass>,
    forces: Option<(Pass, wgpu::Buffer)>,
    planes: (Pass, wgpu::Buffer),
    mean: (Pass, wgpu::Buffer, usize, [usize; 3]),
    parity: usize,
    pub links: Vec<[u32; 2]>,
}

pub struct MeanField {
    pub stride: usize,
    pub size: [usize; 3],
    pub velocity: Vec<Option<[f32; 3]>>,
}

pub struct Fields {
    pub density: Vec<f64>,
    pub velocity: Vec<[f64; 3]>,
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
        let bytes = count as u64 * 4;
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
        for &(cell, direction, q) in &domain.cuts {
            if domain.flags[cell] != FLUID || direction >= Q || !(0.0..=1.0).contains(&q) {
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
        let pipeline = |source: String, entry_point: &str, layouts: &[&wgpu::BindGroupLayout]| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(entry_point),
                source: wgpu::ShaderSource::Wgsl(source.into()),
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
        let common_layout = layout(&[entry(0, uniform), entry(1, read), entry(2, read)]);
        let step = pipeline(
            shader(),
            "step",
            &[&common_layout, &side_layout, &side_layout],
        );
        let common = group(&common_layout, &[&params, &flags, &cuts]);
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
                pipeline: pipeline(edge_shader(), "edge", &[&edge_layout, &side_layout]),
                common: group(&edge_layout, &[&counter, &list]),
                sides: [side(1), side(0)],
                count: n,
                header: counter,
            }
        });
        let mut links: Vec<[u32; 2]> = Vec::new();
        for cell in 0..count {
            if domain.flags[cell] != FLUID {
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
                    (matches!(domain.flags[s], WALL | MOVING) && domain.body[s] > 0)
                        .then_some(domain.body[s])
                })
                .collect();
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
            ]);
            let pass = Pass {
                pipeline: pipeline(force_shader(), "force", &[&force_layout, &side_layout]),
                common: group(&force_layout, &[&header, &flags, &list, &out, &cuts]),
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
            size: coarse_count as u64 * 16,
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
                pipeline: pipeline(mean_shader(), "mean", &[&plane_layout, &side_layout]),
                common: group(&plane_layout, &[&mean_header, &flags, &mean_out]),
                sides: [side(0), side(1)],
                count: coarse_count as u32,
                header: mean_header,
            },
            mean_out,
            stride,
            coarse,
        );
        Ok(Lattice {
            gpu,
            size,
            copies,
            params,
            pipeline: step,
            common,
            sides,
            edges,
            forces,
            planes,
            mean,
            parity: 0,
            links,
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
            let batch = left.min(200);
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            for _ in 0..batch {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.common, &[]);
                pass.set_bind_group(1, &self.sides[self.parity][0], &[]);
                pass.set_bind_group(2, &self.sides[self.parity][1], &[]);
                pass.dispatch_workgroups(gx, gy, 1);
                if let Some(edges) = &self.edges {
                    pass.set_pipeline(&edges.pipeline);
                    pass.set_bind_group(0, &edges.common, &[]);
                    pass.set_bind_group(1, &edges.sides[self.parity], &[]);
                    pass.dispatch_workgroups(edges.count.div_ceil(64), 1, 1);
                }
                drop(pass);
                self.parity ^= 1;
            }
            self.gpu.queue.submit([encoder.finish()]);
            self.gpu
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            left -= batch;
        }
        Ok(())
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
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&pass.pipeline);
            compute.set_bind_group(0, &pass.common, &[]);
            compute.set_bind_group(1, &pass.sides[self.parity], &[]);
            let (gx, gy) = dispatch(count as u32);
            compute.dispatch_workgroups(gx, gy, 1);
        }
        self.gpu.queue.submit([encoder.finish()]);
        let values = self.gpu.read(buffer)?;
        Ok((
            width,
            values
                .chunks_exact(4)
                .take(count)
                .map(|v| [v[0], v[1], v[2], v[3]])
                .collect(),
        ))
    }

    pub fn accumulate(&self) {
        let (pass, ..) = &self.mean;
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&pass.pipeline);
            compute.set_bind_group(0, &pass.common, &[]);
            compute.set_bind_group(1, &pass.sides[self.parity], &[]);
            let (gx, gy) = dispatch(pass.count);
            compute.dispatch_workgroups(gx, gy, 1);
        }
        self.gpu.queue.submit([encoder.finish()]);
    }

    pub fn mean_field(&self) -> Result<MeanField> {
        let (_, buffer, stride, size) = &self.mean;
        let values = self.gpu.read(buffer)?;
        Ok(MeanField {
            stride: *stride,
            size: *size,
            velocity: values
                .chunks_exact(4)
                .map(|v| {
                    if v[3] > 0.0 {
                        Some([v[0] / v[3], v[1] / v[3], v[2] / v[3]])
                    } else {
                        None
                    }
                })
                .collect(),
        })
    }

    pub fn link_samples(&self) -> Result<Vec<[f32; 4]>> {
        let Some((pass, buffer)) = &self.forces else {
            return Ok(Vec::new());
        };
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&pass.pipeline);
            compute.set_bind_group(0, &pass.common, &[]);
            compute.set_bind_group(1, &pass.sides[self.parity], &[]);
            let (gx, gy) = dispatch(pass.count);
            compute.dispatch_workgroups(gx, gy, 1);
        }
        self.gpu.queue.submit([encoder.finish()]);
        let values = self.gpu.read(buffer)?;
        Ok(values
            .chunks_exact(4)
            .map(|v| [v[0], v[1], v[2], v[3]])
            .collect())
    }

    pub fn fields(&self, force: [f64; 3]) -> Result<Fields> {
        let current = &self.copies[self.parity];
        let populations: Vec<Vec<f32>> = current
            .iter()
            .map(|b| self.gpu.read(b))
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
