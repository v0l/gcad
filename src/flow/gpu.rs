use anyhow::{Context, Result, anyhow};

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub name: String,
}

impl Gpu {
    pub fn open() -> Result<Gpu> {
        if cfg!(target_arch = "wasm32") {
            return Err(anyhow!("flow simulations do not run in the browser"));
        }
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
            required_features: adapter.features() & wgpu::Features::SHADER_F16,
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
        let encoder = self.device.create_command_encoder(&Default::default());
        self.take(self.fetch(encoder, buffer))
    }

    pub fn fetch(&self, mut encoder: wgpu::CommandEncoder, buffer: &wgpu::Buffer) -> Reading {
        let size = buffer.size();
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        let index = self.queue.submit([encoder.finish()]);
        Reading { staging, index }
    }

    pub fn fetch_cleared(
        &self,
        mut encoder: wgpu::CommandEncoder,
        buffer: &wgpu::Buffer,
    ) -> Reading {
        let size = buffer.size();
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        encoder.clear_buffer(buffer, 0, None);
        let index = self.queue.submit([encoder.finish()]);
        Reading { staging, index }
    }

    pub fn take(&self, reading: Reading) -> Result<Vec<f32>> {
        let slice = reading.staging.slice(..);
        let (send, receive) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = send.send(r);
        });
        self.wait(reading.index)?;
        receive.recv()?.map_err(|e| anyhow!("{e:?}"))?;
        let data = bytemuck::cast_slice(&slice.get_mapped_range()).to_vec();
        reading.staging.unmap();
        Ok(data)
    }

    pub fn wait(&self, index: wgpu::SubmissionIndex) -> Result<()> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: None,
            })
            .map(|_| ())
            .map_err(|e| anyhow!("{e:?}"))
    }

    pub fn finish(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| anyhow!("{e:?}"))
    }
}

pub struct Reading {
    staging: wgpu::Buffer,
    index: wgpu::SubmissionIndex,
}
