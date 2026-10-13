use super::gpu::Gpu;
use anyhow::Result;

const FMA_ITERATIONS: u32 = 512;
const FMA_CHAINS: u32 = 16;
const WORKGROUP: u32 = 256;

fn fma_shader() -> String {
    let chains: Vec<String> = (0..FMA_CHAINS).map(|k| format!("v{k}")).collect();
    let declare: String = chains
        .iter()
        .enumerate()
        .map(|(k, v)| format!("    var {v} = vec4<f32>(seed + {k}.0, 1.0, 2.0, 3.0);\n"))
        .collect();
    let step: String = chains
        .iter()
        .map(|v| format!("        {v} = fma({v}, m, k);\n"))
        .collect();
    format!(
        "@group(0) @binding(0) var<storage, read_write> sink: array<vec4<f32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {{
    let i = id.x + id.y * groups.x * 256u;
    let seed = f32(i & 1023u) * 1.0e-4;
{declare}    let m = vec4<f32>(0.9999);
    let k = vec4<f32>(0.0001);
    for (var j = 0u; j < {FMA_ITERATIONS}u; j = j + 1u) {{
{step}    }}
    sink[i & 65535u] = {};
}}
",
        chains.join(" + ")
    )
}

const COPY: &str = "
@group(0) @binding(0) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> copied: array<vec4<f32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.x + id.y * groups.x * 256u;
    if (i < arrayLength(&source)) {
        copied[i] = source[i] * 1.0001;
    }
}
";

pub struct Bench {
    pub gpu: String,
    pub tflops: f64,
    pub bandwidth: f64,
}

fn timed(gpu: &Gpu, passes: usize, record: impl Fn(&mut wgpu::ComputePass)) -> Result<f64> {
    let run = |count: usize| -> Result<f64> {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        for _ in 0..count {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            record(&mut pass);
        }
        let started = std::time::Instant::now();
        let index = gpu.queue.submit([encoder.finish()]);
        gpu.wait(index)?;
        Ok(started.elapsed().as_secs_f64())
    };
    run(2)?;
    run(passes)
}

fn pipeline(gpu: &Gpu, source: &str) -> wgpu::ComputePipeline {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bench"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    gpu.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("bench"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
}

fn storage(gpu: &Gpu, size: u64) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bench"),
        size,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    })
}

fn bind(gpu: &Gpu, pipeline: &wgpu::ComputePipeline, buffers: &[&wgpu::Buffer]) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bench"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    })
}

fn fma(gpu: &Gpu) -> Result<f64> {
    let pipeline = pipeline(gpu, &fma_shader());
    let sink = storage(gpu, 65536 * 16);
    let group = bind(gpu, &pipeline, &[&sink]);
    let (gx, gy) = (1024u32, 64u32);
    let passes = 20;
    let seconds = timed(gpu, passes, |pass| {
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(gx, gy, 1);
    })?;
    let threads = f64::from(gx * gy * WORKGROUP);
    let flops = threads * f64::from(FMA_ITERATIONS * FMA_CHAINS) * 4.0 * 2.0 * passes as f64;
    Ok(flops / seconds / 1.0e12)
}

fn copy(gpu: &Gpu) -> Result<f64> {
    let size = (256u64 << 20).min(gpu.max_binding()) / 16 * 16;
    let pipeline = pipeline(gpu, COPY);
    let (source, target) = (storage(gpu, size), storage(gpu, size));
    let group = bind(gpu, &pipeline, &[&source, &target]);
    let groups = (size / 16).div_ceil(u64::from(WORKGROUP)) as u32;
    let (gx, gy) = (256u32, groups.div_ceil(256));
    let passes = 20;
    let seconds = timed(gpu, passes, |pass| {
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(gx, gy, 1);
    })?;
    Ok(2.0 * size as f64 * passes as f64 / seconds / 1.0e9)
}

pub fn run() -> Result<Bench> {
    let gpu = Gpu::open()?;
    let mut bench = Bench {
        gpu: gpu.name.clone(),
        tflops: 0.0,
        bandwidth: 0.0,
    };
    for _ in 0..3 {
        bench.tflops = bench.tflops.max(fma(&gpu)?);
        bench.bandwidth = bench.bandwidth.max(copy(&gpu)?);
    }
    Ok(bench)
}
