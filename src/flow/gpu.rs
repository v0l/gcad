use anyhow::{Context, Result, anyhow};

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub name: String,
}

impl Gpu {
    pub fn open() -> Result<Gpu> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        let want_cpu = std::env::var("GCAD_FLOW_CPU").is_ok();
        let rank = |a: &wgpu::Adapter| {
            let info = a.get_info();
            let kind = match info.device_type {
                wgpu::DeviceType::DiscreteGpu => 0,
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::Cpu => 2,
                _ => 3,
            };
            let backend = if info.backend == wgpu::Backend::Vulkan {
                0
            } else {
                1
            };
            let kind = if want_cpu {
                [2, 1, 0, 3][kind.min(3)]
            } else {
                kind
            };
            (kind, backend)
        };
        let adapter = adapters
            .into_iter()
            .filter(|a| a.limits().max_storage_buffers_per_shader_stage >= 40)
            .min_by_key(rank)
            .ok_or_else(|| {
                anyhow!("no GPU or CPU compute device with 40 storage buffers per shader")
            })?;
        let name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("flow"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .with_context(|| format!("opening {name}"))?;
        Ok(Gpu {
            device,
            queue,
            name,
        })
    }

    pub fn max_binding(&self) -> u64 {
        self.device.limits().max_storage_buffer_binding_size
    }

    pub fn read(&self, buffer: &wgpu::Buffer) -> Result<Vec<f32>> {
        let size = buffer.size();
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (send, receive) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = send.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| anyhow!("{e:?}"))?;
        receive.recv()?.map_err(|e| anyhow!("{e:?}"))?;
        let data = bytemuck::cast_slice(&slice.get_mapped_range()).to_vec();
        staging.unmap();
        Ok(data)
    }
}
