use std::{
    ffi::{CStr, CString},
    mem::size_of,
    ptr,
};

use ash::{Entry, vk};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::scene_mesh::{SceneMesh, Vertex};

const INITIAL_VERTEX_CAPACITY: u64 = 1024 * 1024;
const CLEAR_COLOR: [f32; 4] = [0.08, 0.15, 0.17, 1.0];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniform {
    size: [f32; 2],
    padding: [f32; 2],
}

pub(crate) struct VulkanRenderer {
    device: VulkanDevice,
    instance: VulkanInstance,
    renderer_name: String,
}

impl VulkanRenderer {
    pub(crate) fn new(
        display: Option<RawDisplayHandle>,
        window: RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = VulkanInstance::new(display, window)?;
        let (physical_device, queue_family_index, renderer_name) =
            instance.select_physical_device()?;
        let priorities = [1.0_f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities)];
        let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&device_extensions);
        let logical_device = unsafe {
            instance
                .instance()
                .create_device(physical_device, &device_info, None)
        }
        .map_err(|error| format!("Не удалось создать устройство Vulkan: {error:?}"))?;
        let queue = unsafe { logical_device.get_device_queue(queue_family_index, 0) };
        let swapchain_loader =
            ash::khr::swapchain::Device::new(instance.instance(), &logical_device);
        let mut device = VulkanDevice::new(
            logical_device,
            swapchain_loader,
            queue,
            physical_device,
            queue_family_index,
        );
        device.initialize(&instance, width, height)?;

        Ok(Self {
            device,
            instance,
            renderer_name: format!("Vulkan: {renderer_name}"),
        })
    }

    pub(crate) fn renderer_name(&self) -> &str {
        &self.renderer_name
    }

    pub(crate) fn render(
        &mut self,
        mesh: &SceneMesh,
        surface_width: u32,
        surface_height: u32,
    ) -> Result<(), String> {
        self.device
            .render(&self.instance, mesh, surface_width, surface_height)
    }
}

struct VulkanInstance {
    entry: Entry,
    instance: ash::Instance,
    surface_loader: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
}

impl VulkanInstance {
    fn new(display: Option<RawDisplayHandle>, window: RawWindowHandle) -> Result<Self, String> {
        let required_extensions = required_instance_extensions(display, window)?;
        let entry = unsafe { Entry::load() }
            .map_err(|error| format!("Не удалось загрузить системный Vulkan: {error}"))?;
        let available_extensions =
            unsafe { entry.enumerate_instance_extension_properties(None) }
                .map_err(|error| format!("Не удалось опросить расширения Vulkan: {error:?}"))?;
        for extension in &required_extensions {
            if !available_extensions
                .iter()
                .any(|available| extension_name_matches(available, extension))
            {
                return Err(format!(
                    "Системный Vulkan не предоставляет обязательное расширение {}.",
                    extension.to_string_lossy()
                ));
            }
        }

        let app_name = CString::new("Моря Туманного Севера")
            .expect("Название приложения не должно содержать нулевой байт.");
        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(1)
            .engine_name(&app_name)
            .engine_version(1)
            .api_version(vk::API_VERSION_1_0);
        let extension_names = required_extensions
            .iter()
            .map(|extension| extension.as_ptr())
            .collect::<Vec<_>>();
        let instance_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&extension_names);
        let instance = unsafe { entry.create_instance(&instance_info, None) }
            .map_err(|error| format!("Не удалось создать экземпляр Vulkan: {error:?}"))?;
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = match create_surface(&entry, &instance, display, window) {
            Ok(surface) => surface,
            Err(error) => {
                unsafe { instance.destroy_instance(None) };
                return Err(error);
            }
        };

        Ok(Self {
            entry,
            instance,
            surface_loader,
            surface,
        })
    }

    fn instance(&self) -> &ash::Instance {
        let _keep_loader_loaded = &self.entry;
        &self.instance
    }

    fn select_physical_device(&self) -> Result<(vk::PhysicalDevice, u32, String), String> {
        let physical_devices = unsafe { self.instance.enumerate_physical_devices() }
            .map_err(|error| format!("Не удалось перечислить Vulkan-устройства: {error:?}"))?;
        let swapchain_name = ash::khr::swapchain::NAME;
        let mut candidates = Vec::new();

        for physical_device in physical_devices {
            let properties = unsafe {
                self.instance
                    .get_physical_device_properties(physical_device)
            };
            let extension_properties = unsafe {
                self.instance
                    .enumerate_device_extension_properties(physical_device)
            }
            .map_err(|error| {
                format!("Не удалось прочитать расширения Vulkan-устройства: {error:?}")
            })?;
            if !extension_properties
                .iter()
                .any(|available| extension_name_matches(available, swapchain_name))
            {
                continue;
            }

            let queue_families = unsafe {
                self.instance
                    .get_physical_device_queue_family_properties(physical_device)
            };
            for (index, family) in queue_families.iter().enumerate() {
                if !family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                    continue;
                }
                let can_present = unsafe {
                    self.surface_loader.get_physical_device_surface_support(
                        physical_device,
                        index as u32,
                        self.surface,
                    )
                }
                .map_err(|error| {
                    format!("Не удалось проверить поддержку Vulkan-поверхности: {error:?}")
                })?;
                if !can_present {
                    continue;
                }
                let formats = unsafe {
                    self.surface_loader
                        .get_physical_device_surface_formats(physical_device, self.surface)
                }
                .map_err(|error| {
                    format!("Не удалось опросить форматы поверхности Vulkan: {error:?}")
                })?;
                let present_modes = unsafe {
                    self.surface_loader
                        .get_physical_device_surface_present_modes(physical_device, self.surface)
                }
                .map_err(|error| format!("Не удалось опросить режимы показа Vulkan: {error:?}"))?;
                if formats.is_empty() || !present_modes.contains(&vk::PresentModeKHR::FIFO) {
                    continue;
                }

                let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) }
                    .to_string_lossy()
                    .into_owned();
                let score = match properties.device_type {
                    vk::PhysicalDeviceType::DISCRETE_GPU => 4,
                    vk::PhysicalDeviceType::INTEGRATED_GPU => 3,
                    vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
                    vk::PhysicalDeviceType::CPU => 1,
                    _ => 0,
                };
                candidates.push((score, physical_device, index as u32, name));
                break;
            }
        }

        candidates
            .into_iter()
            .max_by_key(|candidate| candidate.0)
            .map(|(_, physical_device, queue_family, name)| (physical_device, queue_family, name))
            .ok_or_else(|| {
                "Не найдено Vulkan-устройство с поддержкой графики, окна и swapchain.".to_owned()
            })
    }
}

impl Drop for VulkanInstance {
    fn drop(&mut self) {
        unsafe {
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}

fn required_instance_extensions(
    display: Option<RawDisplayHandle>,
    window: RawWindowHandle,
) -> Result<Vec<&'static CStr>, String> {
    match (display, window) {
        (Some(RawDisplayHandle::Xlib(_)), RawWindowHandle::Xlib(_)) => {
            Ok(vec![ash::khr::surface::NAME, ash::khr::xlib_surface::NAME])
        }
        (None, RawWindowHandle::AndroidNdk(_)) => Ok(vec![
            ash::khr::surface::NAME,
            ash::khr::android_surface::NAME,
        ]),
        _ => Err("Неподдерживаемая пара оконных дескрипторов для Vulkan.".to_owned()),
    }
}

fn create_surface(
    entry: &Entry,
    instance: &ash::Instance,
    display: Option<RawDisplayHandle>,
    window: RawWindowHandle,
) -> Result<vk::SurfaceKHR, String> {
    match (display, window) {
        (Some(RawDisplayHandle::Xlib(display)), RawWindowHandle::Xlib(window)) => {
            let display = display
                .display
                .ok_or_else(|| "Дескриптор Xlib не содержит активный дисплей.".to_owned())?;
            let loader = ash::khr::xlib_surface::Instance::new(entry, instance);
            let create_info = vk::XlibSurfaceCreateInfoKHR::default()
                .dpy(display.as_ptr().cast())
                .window(window.window);
            unsafe { loader.create_xlib_surface(&create_info, None) }
                .map_err(|error| format!("Не удалось создать Vulkan-поверхность Xlib: {error:?}"))
        }
        (None, RawWindowHandle::AndroidNdk(window)) => {
            let loader = ash::khr::android_surface::Instance::new(entry, instance);
            let create_info = vk::AndroidSurfaceCreateInfoKHR::default()
                .window(window.a_native_window.as_ptr().cast());
            unsafe { loader.create_android_surface(&create_info, None) }.map_err(|error| {
                format!("Не удалось создать Vulkan-поверхность Android: {error:?}")
            })
        }
        _ => Err("Дескрипторы окна не подходят для Vulkan-поверхности.".to_owned()),
    }
}

fn extension_name_matches(extension: &vk::ExtensionProperties, name: &CStr) -> bool {
    unsafe { CStr::from_ptr(extension.extension_name.as_ptr()) == name }
}

struct BufferAllocation {
    device: ash::Device,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    capacity: vk::DeviceSize,
}

impl Drop for BufferAllocation {
    fn drop(&mut self) {
        unsafe {
            if self.buffer != vk::Buffer::null() {
                self.device.destroy_buffer(self.buffer, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
        }
    }
}

struct VulkanDevice {
    logical: ash::Device,
    swapchain_loader: ash::khr::swapchain::Device,
    queue: vk::Queue,
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
    descriptor_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set: vk::DescriptorSet,
    uniform_buffer: Option<BufferAllocation>,
    vertex_buffer: Option<BufferAllocation>,
    vertex_capacity: u64,
    swapchain: Option<SwapchainResources>,
}

impl VulkanDevice {
    fn new(
        logical: ash::Device,
        swapchain_loader: ash::khr::swapchain::Device,
        queue: vk::Queue,
        physical_device: vk::PhysicalDevice,
        queue_family_index: u32,
    ) -> Self {
        Self {
            logical,
            swapchain_loader,
            queue,
            physical_device,
            queue_family_index,
            descriptor_layout: vk::DescriptorSetLayout::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            descriptor_set: vk::DescriptorSet::null(),
            uniform_buffer: None,
            vertex_buffer: None,
            vertex_capacity: 0,
            swapchain: None,
        }
    }

    fn initialize(
        &mut self,
        instance: &VulkanInstance,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let layout_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::VERTEX)];
        let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&layout_binding);
        self.descriptor_layout = unsafe {
            self.logical
                .create_descriptor_set_layout(&layout_info, None)
        }
        .map_err(|error| format!("Не удалось создать компоновку дескрипторов Vulkan: {error:?}"))?;

        self.uniform_buffer = Some(create_buffer(
            &self.logical,
            instance,
            self.physical_device,
            size_of::<ViewportUniform>() as u64,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
        )?);
        self.vertex_buffer = Some(create_buffer(
            &self.logical,
            instance,
            self.physical_device,
            INITIAL_VERTEX_CAPACITY,
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?);
        self.vertex_capacity = INITIAL_VERTEX_CAPACITY;

        let pool_size = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(1)
            .pool_sizes(&pool_size);
        self.descriptor_pool = unsafe { self.logical.create_descriptor_pool(&pool_info, None) }
            .map_err(|error| format!("Не удалось создать пул дескрипторов Vulkan: {error:?}"))?;
        let layouts = [self.descriptor_layout];
        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&layouts);
        self.descriptor_set = unsafe { self.logical.allocate_descriptor_sets(&allocate_info) }
            .map_err(|error| format!("Не удалось выделить дескриптор Vulkan: {error:?}"))?[0];
        let uniform = self.uniform_buffer.as_ref().unwrap();
        let buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(uniform.buffer)
            .offset(0)
            .range(size_of::<ViewportUniform>() as u64)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(self.descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&buffer_info)];
        unsafe { self.logical.update_descriptor_sets(&write, &[]) };
        self.recreate_swapchain(instance, width, height)
    }

    fn render(
        &mut self,
        instance: &VulkanInstance,
        mesh: &SceneMesh,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);
        if self.swapchain.as_ref().is_none_or(|swapchain| {
            swapchain.extent.width != width || swapchain.extent.height != height
        }) {
            self.recreate_swapchain(instance, width, height)?;
        }

        let viewport = ViewportUniform {
            size: [mesh.width.max(1.0), mesh.height.max(1.0)],
            padding: [0.0; 2],
        };
        write_buffer(
            &self.logical,
            self.uniform_buffer.as_ref().unwrap(),
            bytemuck::bytes_of(&viewport),
        )?;

        let vertex_bytes = bytemuck::cast_slice::<Vertex, u8>(&mesh.vertices);
        if vertex_bytes.len() as u64 > self.vertex_capacity {
            unsafe { self.logical.device_wait_idle() }
                .map_err(|error| format!("Не удалось дождаться кадра Vulkan: {error:?}"))?;
            self.vertex_buffer.take();
            self.vertex_capacity = (vertex_bytes.len() as u64).next_power_of_two();
            self.vertex_buffer = Some(create_buffer(
                &self.logical,
                instance,
                self.physical_device,
                self.vertex_capacity,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?);
        }
        if !vertex_bytes.is_empty() {
            write_buffer(
                &self.logical,
                self.vertex_buffer.as_ref().unwrap(),
                vertex_bytes,
            )?;
        }

        let resources = self.swapchain.as_ref().unwrap();
        unsafe {
            self.logical
                .wait_for_fences(&[resources.frame_fence], true, u64::MAX)
        }
        .map_err(|error| format!("Не удалось дождаться готовности Vulkan-кадра: {error:?}"))?;
        let (image_index, was_suboptimal) = match unsafe {
            self.swapchain_loader.acquire_next_image(
                resources.swapchain,
                u64::MAX,
                resources.image_available,
                vk::Fence::null(),
            )
        } {
            Ok(result) => result,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.recreate_swapchain(instance, width, height)?;
                return Ok(());
            }
            Err(error) => return Err(format!("Не удалось получить изображение Vulkan: {error:?}")),
        };

        unsafe { self.logical.reset_fences(&[resources.frame_fence]) }
            .map_err(|error| format!("Не удалось сбросить Vulkan-синхронизацию: {error:?}"))?;
        unsafe {
            self.logical.reset_command_buffer(
                resources.command_buffer,
                vk::CommandBufferResetFlags::empty(),
            )
        }
        .map_err(|error| format!("Не удалось сбросить командный буфер Vulkan: {error:?}"))?;

        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.logical
                .begin_command_buffer(resources.command_buffer, &begin_info)
        }
        .map_err(|error| format!("Не удалось начать запись Vulkan-кадра: {error:?}"))?;
        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: CLEAR_COLOR,
            },
        }];
        let render_area = vk::Rect2D::default().extent(resources.extent);
        let render_pass_info = vk::RenderPassBeginInfo::default()
            .render_pass(resources.render_pass)
            .framebuffer(resources.framebuffers[image_index as usize])
            .render_area(render_area)
            .clear_values(&clear_values);
        unsafe {
            self.logical.cmd_begin_render_pass(
                resources.command_buffer,
                &render_pass_info,
                vk::SubpassContents::INLINE,
            );
            self.logical.cmd_bind_pipeline(
                resources.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                resources.pipeline,
            );
            self.logical.cmd_bind_descriptor_sets(
                resources.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                resources.pipeline_layout,
                0,
                &[self.descriptor_set],
                &[],
            );
            self.logical.cmd_bind_vertex_buffers(
                resources.command_buffer,
                0,
                &[self.vertex_buffer.as_ref().unwrap().buffer],
                &[0],
            );
            self.logical.cmd_draw(
                resources.command_buffer,
                mesh.vertices.len() as u32,
                1,
                0,
                0,
            );
            self.logical.cmd_end_render_pass(resources.command_buffer);
            self.logical.end_command_buffer(resources.command_buffer)
        }
        .map_err(|error| format!("Не удалось завершить запись Vulkan-кадра: {error:?}"))?;

        let wait_semaphores = [resources.image_available];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let command_buffers = [resources.command_buffer];
        let signal_semaphores = [resources.render_finished[image_index as usize]];
        let submit_info = [vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&command_buffers)
            .signal_semaphores(&signal_semaphores)];
        unsafe {
            self.logical
                .queue_submit(self.queue, &submit_info, resources.frame_fence)
        }
        .map_err(|error| format!("Не удалось отправить Vulkan-кадр: {error:?}"))?;

        let swapchains = [resources.swapchain];
        let image_indices = [image_index];
        let present_wait = [resources.render_finished[image_index as usize]];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&present_wait)
            .swapchains(&swapchains)
            .image_indices(&image_indices);
        let presented_suboptimally = match unsafe {
            self.swapchain_loader
                .queue_present(self.queue, &present_info)
        } {
            Ok(suboptimal) => suboptimal,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => true,
            Err(error) => return Err(format!("Не удалось показать Vulkan-кадр: {error:?}")),
        };
        if was_suboptimal || presented_suboptimally {
            self.recreate_swapchain(instance, width, height)?;
        }
        Ok(())
    }

    fn recreate_swapchain(
        &mut self,
        instance: &VulkanInstance,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        unsafe { self.logical.device_wait_idle() }
            .map_err(|error| format!("Не удалось дождаться завершения Vulkan: {error:?}"))?;
        self.swapchain.take();
        self.swapchain = Some(SwapchainResources::new(
            &self.logical,
            &self.swapchain_loader,
            &instance.surface_loader,
            instance.surface,
            self.physical_device,
            self.queue_family_index,
            self.descriptor_layout,
            width,
            height,
        )?);
        Ok(())
    }
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = self.logical.device_wait_idle();
            self.swapchain.take();
            if self.descriptor_pool != vk::DescriptorPool::null() {
                self.logical
                    .destroy_descriptor_pool(self.descriptor_pool, None);
            }
            if self.descriptor_layout != vk::DescriptorSetLayout::null() {
                self.logical
                    .destroy_descriptor_set_layout(self.descriptor_layout, None);
            }
            self.vertex_buffer.take();
            self.uniform_buffer.take();
            self.logical.destroy_device(None);
        }
    }
}

struct SwapchainResources {
    device: ash::Device,
    loader: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    extent: vk::Extent2D,
    image_views: Vec<vk::ImageView>,
    render_pass: vk::RenderPass,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    framebuffers: Vec<vk::Framebuffer>,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available: vk::Semaphore,
    render_finished: Vec<vk::Semaphore>,
    frame_fence: vk::Fence,
}

impl SwapchainResources {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &ash::Device,
        swapchain_loader: &ash::khr::swapchain::Device,
        surface_loader: &ash::khr::surface::Instance,
        surface: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
        queue_family_index: u32,
        descriptor_layout: vk::DescriptorSetLayout,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let mut resources = Self {
            device: device.clone(),
            loader: swapchain_loader.clone(),
            swapchain: vk::SwapchainKHR::null(),
            extent: vk::Extent2D::default(),
            image_views: Vec::new(),
            render_pass: vk::RenderPass::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            framebuffers: Vec::new(),
            command_pool: vk::CommandPool::null(),
            command_buffer: vk::CommandBuffer::null(),
            image_available: vk::Semaphore::null(),
            render_finished: Vec::new(),
            frame_fence: vk::Fence::null(),
        };
        let capabilities = unsafe {
            surface_loader.get_physical_device_surface_capabilities(physical_device, surface)
        }
        .map_err(|error| format!("Не удалось прочитать параметры Vulkan-поверхности: {error:?}"))?;
        let formats =
            unsafe { surface_loader.get_physical_device_surface_formats(physical_device, surface) }
                .map_err(|error| {
                    format!("Не удалось прочитать форматы Vulkan-поверхности: {error:?}")
                })?;
        let present_modes = unsafe {
            surface_loader.get_physical_device_surface_present_modes(physical_device, surface)
        }
        .map_err(|error| format!("Не удалось прочитать режимы показа Vulkan: {error:?}"))?;
        let surface_format = choose_surface_format(&formats)
            .ok_or_else(|| "У Vulkan-поверхности нет подходящего формата цвета.".to_owned())?;
        if !present_modes.contains(&vk::PresentModeKHR::FIFO) {
            return Err("Vulkan-поверхность не поддерживает обязательный режим FIFO.".to_owned());
        }
        let extent = choose_extent(&capabilities, width, height);
        let image_count = choose_image_count(&capabilities);
        let composite_alpha = choose_composite_alpha(capabilities.supported_composite_alpha)
            .ok_or_else(|| "У Vulkan-поверхности нет режима альфа-композиции.".to_owned())?;
        let queue_family_indices = [queue_family_index];
        let swapchain_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .queue_family_indices(&queue_family_indices)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(composite_alpha)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true);
        resources.swapchain =
            unsafe { resources.loader.create_swapchain(&swapchain_info, None) }
                .map_err(|error| format!("Не удалось создать Vulkan-swapchain: {error:?}"))?;
        resources.extent = extent;
        let images = unsafe { resources.loader.get_swapchain_images(resources.swapchain) }
            .map_err(|error| {
                format!("Не удалось получить изображения Vulkan-swapchain: {error:?}")
            })?;
        for image in images {
            let subresource = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1);
            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(surface_format.format)
                .subresource_range(subresource);
            let view = unsafe { resources.device.create_image_view(&view_info, None) }
                .map_err(|error| format!("Не удалось создать Vulkan-вид изображения: {error:?}"))?;
            resources.image_views.push(view);
        }
        resources.render_pass = create_render_pass(&resources.device, surface_format.format)?;
        let (pipeline_layout, pipeline) = create_pipeline(
            &resources.device,
            resources.render_pass,
            descriptor_layout,
            extent,
        )?;
        resources.pipeline_layout = pipeline_layout;
        resources.pipeline = pipeline;
        for view in &resources.image_views {
            let attachments = [*view];
            let framebuffer_info = vk::FramebufferCreateInfo::default()
                .render_pass(resources.render_pass)
                .attachments(&attachments)
                .width(extent.width)
                .height(extent.height)
                .layers(1);
            let framebuffer =
                unsafe { resources.device.create_framebuffer(&framebuffer_info, None) }
                    .map_err(|error| format!("Не удалось создать Vulkan-кадр: {error:?}"))?;
            resources.framebuffers.push(framebuffer);
        }

        let command_pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        resources.command_pool = unsafe {
            resources
                .device
                .create_command_pool(&command_pool_info, None)
        }
        .map_err(|error| format!("Не удалось создать пул команд Vulkan: {error:?}"))?;
        let command_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(resources.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        resources.command_buffer =
            unsafe { resources.device.allocate_command_buffers(&command_info) }.map_err(
                |error| format!("Не удалось выделить командный буфер Vulkan: {error:?}"),
            )?[0];

        resources.image_available = create_semaphore(&resources.device)?;
        for _ in &resources.image_views {
            resources
                .render_finished
                .push(create_semaphore(&resources.device)?);
        }
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        resources.frame_fence = unsafe { resources.device.create_fence(&fence_info, None) }
            .map_err(|error| format!("Не удалось создать Vulkan-синхронизацию: {error:?}"))?;
        Ok(resources)
    }
}

impl Drop for SwapchainResources {
    fn drop(&mut self) {
        unsafe {
            for framebuffer in self.framebuffers.drain(..) {
                self.device.destroy_framebuffer(framebuffer, None);
            }
            if self.pipeline != vk::Pipeline::null() {
                self.device.destroy_pipeline(self.pipeline, None);
            }
            if self.pipeline_layout != vk::PipelineLayout::null() {
                self.device
                    .destroy_pipeline_layout(self.pipeline_layout, None);
            }
            if self.render_pass != vk::RenderPass::null() {
                self.device.destroy_render_pass(self.render_pass, None);
            }
            for view in self.image_views.drain(..) {
                self.device.destroy_image_view(view, None);
            }
            if self.swapchain != vk::SwapchainKHR::null() {
                self.loader.destroy_swapchain(self.swapchain, None);
            }
            if self.command_pool != vk::CommandPool::null() {
                self.device.destroy_command_pool(self.command_pool, None);
            }
            if self.image_available != vk::Semaphore::null() {
                self.device.destroy_semaphore(self.image_available, None);
            }
            for semaphore in self.render_finished.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }
            if self.frame_fence != vk::Fence::null() {
                self.device.destroy_fence(self.frame_fence, None);
            }
        }
    }
}

fn create_buffer(
    device: &ash::Device,
    instance: &VulkanInstance,
    physical_device: vk::PhysicalDevice,
    capacity: u64,
    usage: vk::BufferUsageFlags,
) -> Result<BufferAllocation, String> {
    let buffer_info = vk::BufferCreateInfo::default()
        .size(capacity)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe { device.create_buffer(&buffer_info, None) }
        .map_err(|error| format!("Не удалось создать Vulkan-буфер: {error:?}"))?;
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let memory_properties = unsafe {
        instance
            .instance
            .get_physical_device_memory_properties(physical_device)
    };
    let memory_type = (0..memory_properties.memory_type_count)
        .find(|index| {
            let type_supported = requirements.memory_type_bits & (1 << index) != 0;
            let flags = memory_properties.memory_types[*index as usize].property_flags;
            type_supported
                && flags.contains(
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
        })
        .ok_or_else(|| {
            "У Vulkan-устройства нет доступной общей памяти для игровых вершин.".to_owned()
        });
    let memory_type = match memory_type {
        Ok(memory_type) => memory_type,
        Err(error) => {
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(error);
        }
    };
    let allocation_info = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    let memory = match unsafe { device.allocate_memory(&allocation_info, None) } {
        Ok(memory) => memory,
        Err(error) => {
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(format!("Не удалось выделить память Vulkan: {error:?}"));
        }
    };
    if let Err(error) = unsafe { device.bind_buffer_memory(buffer, memory, 0) } {
        unsafe {
            device.free_memory(memory, None);
            device.destroy_buffer(buffer, None);
        }
        return Err(format!(
            "Не удалось связать память Vulkan-буфера: {error:?}"
        ));
    }
    Ok(BufferAllocation {
        device: device.clone(),
        buffer,
        memory,
        capacity,
    })
}

fn write_buffer(
    device: &ash::Device,
    allocation: &BufferAllocation,
    bytes: &[u8],
) -> Result<(), String> {
    if bytes.len() as u64 > allocation.capacity {
        return Err("Размер данных превышает ёмкость Vulkan-буфера.".to_owned());
    }
    let mapped = unsafe {
        device.map_memory(
            allocation.memory,
            0,
            bytes.len() as u64,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|error| format!("Не удалось открыть память Vulkan-буфера: {error:?}"))?;
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.cast::<u8>(), bytes.len());
        device.unmap_memory(allocation.memory);
    }
    Ok(())
}

fn choose_surface_format(formats: &[vk::SurfaceFormatKHR]) -> Option<vk::SurfaceFormatKHR> {
    if formats.len() == 1 && formats[0].format == vk::Format::UNDEFINED {
        return Some(vk::SurfaceFormatKHR {
            format: vk::Format::B8G8R8A8_UNORM,
            color_space: formats[0].color_space,
        });
    }
    formats
        .iter()
        .copied()
        .find(|format| {
            matches!(
                format.format,
                vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
            ) && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .or_else(|| formats.first().copied())
}

fn choose_extent(
    capabilities: &vk::SurfaceCapabilitiesKHR,
    requested_width: u32,
    requested_height: u32,
) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        return capabilities.current_extent;
    }
    vk::Extent2D {
        width: requested_width.clamp(
            capabilities.min_image_extent.width,
            capabilities.max_image_extent.width,
        ),
        height: requested_height.clamp(
            capabilities.min_image_extent.height,
            capabilities.max_image_extent.height,
        ),
    }
}

fn choose_image_count(capabilities: &vk::SurfaceCapabilitiesKHR) -> u32 {
    let requested = capabilities.min_image_count.saturating_add(1);
    if capabilities.max_image_count == 0 {
        requested
    } else {
        requested.min(capabilities.max_image_count)
    }
}

fn choose_composite_alpha(
    supported: vk::CompositeAlphaFlagsKHR,
) -> Option<vk::CompositeAlphaFlagsKHR> {
    [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::INHERIT,
    ]
    .into_iter()
    .find(|flag| supported.contains(*flag))
}

fn create_semaphore(device: &ash::Device) -> Result<vk::Semaphore, String> {
    unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
        .map_err(|error| format!("Не удалось создать семафор Vulkan: {error:?}"))
}

fn create_render_pass(device: &ash::Device, format: vk::Format) -> Result<vk::RenderPass, String> {
    let attachments = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];
    let color_attachments = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachments)];
    let dependencies = [vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    unsafe { device.create_render_pass(&info, None) }
        .map_err(|error| format!("Не удалось создать Vulkan render pass: {error:?}"))
}

fn create_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    descriptor_layout: vk::DescriptorSetLayout,
    extent: vk::Extent2D,
) -> Result<(vk::PipelineLayout, vk::Pipeline), String> {
    let set_layouts = [descriptor_layout];
    let layout_info = vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts);
    let pipeline_layout = unsafe { device.create_pipeline_layout(&layout_info, None) }
        .map_err(|error| format!("Не удалось создать Vulkan pipeline layout: {error:?}"))?;

    let vertex_words = spirv_words(include_bytes!(concat!(env!("OUT_DIR"), "/scene.vert.spv")));
    let fragment_words = spirv_words(include_bytes!(concat!(env!("OUT_DIR"), "/scene.frag.spv")));
    let vertex_info = vk::ShaderModuleCreateInfo::default().code(&vertex_words);
    let vertex_shader = match unsafe { device.create_shader_module(&vertex_info, None) } {
        Ok(shader) => shader,
        Err(error) => {
            unsafe { device.destroy_pipeline_layout(pipeline_layout, None) };
            return Err(format!(
                "Не удалось загрузить Vulkan-вершинный шейдер: {error:?}"
            ));
        }
    };
    let fragment_info = vk::ShaderModuleCreateInfo::default().code(&fragment_words);
    let fragment_shader = match unsafe { device.create_shader_module(&fragment_info, None) } {
        Ok(shader) => shader,
        Err(error) => {
            unsafe {
                device.destroy_shader_module(vertex_shader, None);
                device.destroy_pipeline_layout(pipeline_layout, None);
            }
            return Err(format!(
                "Не удалось загрузить Vulkan-фрагментный шейдер: {error:?}"
            ));
        }
    };

    let entry_vertex = CString::new("vs_main").unwrap();
    let entry_fragment = CString::new("fs_main").unwrap();
    let shader_stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vertex_shader)
            .name(&entry_vertex),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment_shader)
            .name(&entry_fragment),
    ];
    let bindings = [vk::VertexInputBindingDescription::default()
        .binding(0)
        .stride(size_of::<Vertex>() as u32)
        .input_rate(vk::VertexInputRate::VERTEX)];
    let attributes = [
        vk::VertexInputAttributeDescription::default()
            .location(0)
            .binding(0)
            .format(vk::Format::R32G32_SFLOAT)
            .offset(0),
        vk::VertexInputAttributeDescription::default()
            .location(1)
            .binding(0)
            .format(vk::Format::R32G32B32A32_SFLOAT)
            .offset(size_of::<[f32; 2]>() as u32),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
        .primitive_restart_enable(false);
    let viewports = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: extent.width as f32,
        height: extent.height as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    }];
    let scissors = [vk::Rect2D::default().extent(extent)];
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewports(&viewports)
        .scissors(&scissors);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(false)
        .rasterizer_discard_enable(false)
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .depth_bias_enable(false)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let blend_attachments = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
    let pipeline_infos = [vk::GraphicsPipelineCreateInfo::default()
        .stages(&shader_stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .layout(pipeline_layout)
        .render_pass(render_pass)
        .subpass(0)];
    let result = unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_infos, None)
    };
    unsafe {
        device.destroy_shader_module(vertex_shader, None);
        device.destroy_shader_module(fragment_shader, None);
    }
    match result {
        Ok(pipelines) => Ok((pipeline_layout, pipelines[0])),
        Err((pipelines, error)) => {
            unsafe {
                for pipeline in pipelines {
                    device.destroy_pipeline(pipeline, None);
                }
                device.destroy_pipeline_layout(pipeline_layout, None);
            }
            Err(format!(
                "Не удалось создать Vulkan graphics pipeline: {error:?}"
            ))
        }
    }
}

fn spirv_words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(size_of::<u32>())
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_spirv_modules_have_valid_headers() {
        let vertex = spirv_words(include_bytes!(concat!(env!("OUT_DIR"), "/scene.vert.spv")));
        let fragment = spirv_words(include_bytes!(concat!(env!("OUT_DIR"), "/scene.frag.spv")));
        assert_eq!(vertex.first(), Some(&0x0723_0203));
        assert_eq!(fragment.first(), Some(&0x0723_0203));
    }

    #[test]
    fn surface_size_is_limited_to_supported_extent() {
        let capabilities = vk::SurfaceCapabilitiesKHR {
            min_image_extent: vk::Extent2D {
                width: 100,
                height: 100,
            },
            max_image_extent: vk::Extent2D {
                width: 900,
                height: 700,
            },
            current_extent: vk::Extent2D {
                width: u32::MAX,
                height: u32::MAX,
            },
            ..Default::default()
        };
        assert_eq!(
            choose_extent(&capabilities, 1200, 40),
            vk::Extent2D {
                width: 900,
                height: 100,
            }
        );
    }
}
