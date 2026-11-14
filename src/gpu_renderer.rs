use std::borrow::Cow;

#[cfg(not(target_arch = "wasm32"))]
use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle};

use crate::scene_mesh::{SceneMesh, Vertex};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct NativeDisplayHandle(RawDisplayHandle);

#[cfg(not(target_arch = "wasm32"))]
// Безопасность обеспечивается платформенным владельцем дисплея и XInitThreads на Linux.
unsafe impl Send for NativeDisplayHandle {}

#[cfg(not(target_arch = "wasm32"))]
// Безопасность обеспечивается платформенным владельцем дисплея и XInitThreads на Linux.
unsafe impl Sync for NativeDisplayHandle {}

#[cfg(not(target_arch = "wasm32"))]
impl HasDisplayHandle for NativeDisplayHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // Дескриптор остаётся действительным, пока существует нативное окно и рендерер.
        Ok(unsafe { DisplayHandle::borrow_raw(self.0) })
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniform {
    size: [f32; 2],
    padding: [f32; 2],
}

pub(crate) struct GpuRenderer {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: u64,
    configuration: wgpu::SurfaceConfiguration,
    #[cfg(not(target_arch = "wasm32"))]
    renderer_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    adapter_vendor_id: u32,
    #[cfg(not(target_arch = "wasm32"))]
    adapter_name: String,
}

impl GpuRenderer {
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn new_web(canvas: web_sys::HtmlCanvasElement) -> Result<Self, String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|error| format!("Не удалось создать поверхность WebGPU: {error}"))?;
        Self::from_surface(instance, surface, Some(wgpu::Backend::BrowserWebGpu)).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn new_native_webgpu(
        display: Option<raw_window_handle::RawDisplayHandle>,
        window: raw_window_handle::RawWindowHandle,
    ) -> Result<Self, String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
        descriptor.display = Some(Box::new(NativeDisplayHandle(display.ok_or_else(|| {
            "Для WebGPU необходим дескриптор нативного дисплея.".to_owned()
        })?)));
        let instance = wgpu::Instance::new(descriptor);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: display,
                raw_window_handle: window,
            })
        }
        .map_err(|error| {
            format!("Не удалось создать поверхность WebGPU для нативного окна: {error}")
        })?;
        pollster::block_on(Self::from_surface(instance, surface, None))
    }

    async fn from_surface(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        required_backend: Option<wgpu::Backend>,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("Не найден графический адаптер: {error}"))?;
        let adapter_info = adapter.get_info();
        if let Some(required_backend) = required_backend {
            if adapter_info.backend != required_backend {
                return Err(format!(
                    "Выбран неподходящий графический API: {:?}; требуется {:?}.",
                    adapter_info.backend, required_backend
                ));
            }
        }

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Моря Туманного Севера"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("Не удалось открыть графическое устройство: {error}"))?;

        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Параметры области сцены"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(
                        std::mem::size_of::<ViewportUniform>() as u64,
                    ),
                },
                count: None,
            }],
        });
        let viewport_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Размер области сцены"),
            size: std::mem::size_of::<ViewportUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Параметры преобразования сцены"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Компоновка рендерера сцены"),
            bind_group_layouts: &[Some(&viewport_layout)],
            immediate_size: 0,
        });

        let mut configuration = surface
            .get_default_config(&adapter, 1200, 700)
            .ok_or_else(|| "Графический адаптер не поддерживает поверхность окна.".to_owned())?;
        configuration.desired_maximum_frame_latency = 2;
        surface.configure(&device, &configuration);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Шейдер сцены"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SCENE_SHADER)),
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        };
        let color_target = Some(wgpu::ColorTargetState {
            format: configuration.format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Рендер игровой сцены"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout)],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[color_target],
            }),
            multiview_mask: None,
            cache: None,
        });

        let vertex_capacity = 16 * 1024;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Вершины игровой сцены"),
            size: vertex_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            pipeline,
            viewport_buffer,
            viewport_bind_group,
            vertex_buffer,
            vertex_capacity,
            configuration,
            #[cfg(not(target_arch = "wasm32"))]
            renderer_name: format!("WebGPU: {} ({:?})", adapter_info.name, adapter_info.backend),
            #[cfg(not(target_arch = "wasm32"))]
            adapter_vendor_id: adapter_info.vendor,
            #[cfg(not(target_arch = "wasm32"))]
            adapter_name: adapter_info.name.clone(),
        })
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.configuration.width == width && self.configuration.height == height {
            return;
        }
        self.configuration.width = width;
        self.configuration.height = height;
        self.surface.configure(&self.device, &self.configuration);
    }

    pub(crate) fn render(
        &mut self,
        mesh: &SceneMesh,
        surface_width: u32,
        surface_height: u32,
    ) -> Result<(), String> {
        let viewport = ViewportUniform {
            size: [mesh.width.max(1.0), mesh.height.max(1.0)],
            padding: [0.0; 2],
        };
        self.queue
            .write_buffer(&self.viewport_buffer, 0, bytemuck::bytes_of(&viewport));
        let bytes: &[u8] = bytemuck::cast_slice(&mesh.vertices);
        if bytes.len() as u64 > self.vertex_capacity {
            self.vertex_capacity = (bytes.len() as u64).next_power_of_two();
            self.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Вершины игровой сцены"),
                size: self.vertex_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.vertex_buffer, 0, bytes);
        }
        self.resize(surface_width, surface_height);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.configuration);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                    other => {
                        return Err(format!("Не удалось получить кадр Vulkan/WebGPU: {other:?}"));
                    }
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            other => return Err(format!("Не удалось получить кадр Vulkan/WebGPU: {other:?}")),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Кадр игровой сцены"),
            });
        {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.08,
                        g: 0.15,
                        b: 0.17,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Рисование игровой сцены"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(0..mesh.vertices.len() as u32, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn renderer_name(&self) -> &str {
        &self.renderer_name
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn adapter_vendor_id(&self) -> u32 {
        self.adapter_vendor_id
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn adapter_name(&self) -> &str {
        &self.adapter_name
    }
}

const SCENE_SHADER: &str = include_str!("../assets/shaders/scene.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_shader_is_valid_wgsl() {
        let module =
            naga::front::wgsl::parse_str(SCENE_SHADER).expect("Шейдер должен разбираться.");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Шейдер должен проходить проверку WGSL.");
    }
}
