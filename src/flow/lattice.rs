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

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub viscosity: f64,
    pub force: [f64; 3],
    pub wall: [f64; 3],
    pub collision: Collision,
    pub smagorinsky: f64,
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
    pad0: f32,
    pad1: f32,
    pad2: f32,
}

fn shader() -> String {
    let mut s = String::new();
    s += "struct Params { nx: u32, ny: u32, nz: u32, count: u32, omega: f32, fx: f32, fy: f32, fz: f32, wx: f32, wy: f32, wz: f32, regular: f32, smagorinsky: f32, pad0: f32, pad1: f32, pad2: f32 }\n";
    s += "@group(0) @binding(0) var<uniform> p: Params;\n";
    s += "@group(0) @binding(1) var<storage, read> flags: array<u32>;\n";
    for i in 0..Q {
        s += &format!("@group(1) @binding({i}) var<storage, read_write> a{i}: array<f32>;\n");
        s += &format!("@group(2) @binding({i}) var<storage, read_write> b{i}: array<f32>;\n");
    }
    s += "fn wrap(v: i32, n: u32) -> u32 { let m = i32(n); return u32(((v % m) + m) % m); }\n";
    s += "@compute @workgroup_size(64)\n";
    s += "fn step(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n";
    s += "  let index = id.x + id.y * groups.x * 64u;\n";
    s += "  if (index >= p.count) { return; }\n";
    s += "  if (flags[index] != 0u) { return; }\n";
    s += "  let x = i32(index % p.nx); let y = i32((index / p.nx) % p.ny); let z = i32(index / (p.nx * p.ny));\n";
    s += "  var s: u32; var fs: u32;\n";
    for (i, c) in C.iter().enumerate() {
        let w = weight(i);
        let o = opposite(i);
        s += &format!(
            "  s = wrap(x - ({}), p.nx) + (wrap(y - ({}), p.ny) + wrap(z - ({}), p.nz) * p.ny) * p.nx;\n",
            c[0], c[1], c[2]
        );
        s += "  fs = flags[s];\n";
        s += &format!(
            "  var g{i}: f32 = a{i}[s];\n  if (fs != 0u) {{ g{i} = a{o}[index]; if (fs == 2u) {{ g{i} = g{i} + {:.17} * (({}.0) * p.wx + ({}.0) * p.wy + ({}.0) * p.wz); }} }}\n",
            6.0 * w,
            c[0],
            c[1],
            c[2]
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
            "  let p{name} = 0.0 {}{iso} - rho * {} * {} + 0.5 * ({} * p.f{} + {} * p.f{});\n",
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
            "  {{ let cu = ({}.0) * ux + ({}.0) * uy + ({}.0) * uz;\n    let eq = {w:.17} * (rho - 1.0 + rho * (3.0 * cu + 4.5 * cu * cu - 1.5 * uu));\n    let push = keep * {w:.17} * ((3.0 * (({}.0) - ux) + 9.0 * cu * ({}.0)) * p.fx + (3.0 * (({}.0) - uy) + 9.0 * cu * ({}.0)) * p.fy + (3.0 * (({}.0) - uz) + 9.0 * cu * ({}.0)) * p.fz);\n    let free = select(g{i} - eq, {:.17} * {projection}, p.regular > 0.5);\n    b{i}[index] = eq + (1.0 - omega) * free + push; }}\n",
            c[0],
            c[1],
            c[2],
            c[0],
            c[0],
            c[1],
            c[1],
            c[2],
            c[2],
            4.5 * w
        );
    }
    s += "}\n";
    s
}

pub struct Lattice<'a> {
    gpu: &'a Gpu,
    pub size: [usize; 3],
    copies: [Vec<wgpu::Buffer>; 2],
    params: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    common: wgpu::BindGroup,
    sides: [[wgpu::BindGroup; 2]; 2],
    parity: usize,
}

pub struct Fields {
    pub density: Vec<f64>,
    pub velocity: Vec<[f64; 3]>,
}

impl<'a> Lattice<'a> {
    pub fn new(gpu: &'a Gpu, size: [usize; 3], flags: &[u8]) -> Result<Lattice<'a>> {
        let count = size[0] * size[1] * size[2];
        if flags.len() != count {
            bail!("{} flags for {count} cells", flags.len());
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
        let flag_words: Vec<u32> = flags.iter().map(|&f| f as u32).collect();
        let flags = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("flags"),
            contents: bytemuck::cast_slice(&flag_words),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lattice"),
            source: wgpu::ShaderSource::Wgsl(shader().into()),
        });
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let common_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("common"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, true),
            ],
        });
        let side_entries: Vec<wgpu::BindGroupLayoutEntry> =
            (0..Q as u32).map(|i| storage(i, false)).collect();
        let side_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("populations"),
            entries: &side_entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lattice"),
            bind_group_layouts: &[Some(&common_layout), Some(&side_layout), Some(&side_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("lattice"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("step"),
            compilation_options: Default::default(),
            cache: None,
        });
        let common = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("common"),
            layout: &common_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: flags.as_entire_binding(),
                },
            ],
        });
        let side = |buffers: &Vec<wgpu::Buffer>| {
            let entries: Vec<wgpu::BindGroupEntry> = buffers
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("populations"),
                layout: &side_layout,
                entries: &entries,
            })
        };
        let sides = [
            [side(&copies[0]), side(&copies[1])],
            [side(&copies[1]), side(&copies[0])],
        ];
        Ok(Lattice {
            gpu,
            size,
            copies,
            params,
            pipeline,
            common,
            sides,
            parity: 0,
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
            pad0: 0.0,
            pad1: 0.0,
            pad2: 0.0,
        };
        self.gpu
            .queue
            .write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        let groups = count.div_ceil(64);
        let (gx, gy) = if groups <= 65535 {
            (groups, 1)
        } else {
            (65535, groups.div_ceil(65535))
        };
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
