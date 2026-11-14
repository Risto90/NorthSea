use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{gpu_renderer::GpuRenderer, scene_mesh::SceneMesh, vulkan_renderer::VulkanRenderer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeGraphicsApi {
    Vulkan,
    WebGpu,
}

fn select_native_graphics_api(vulkan_available: bool) -> NativeGraphicsApi {
    if vulkan_available {
        NativeGraphicsApi::Vulkan
    } else {
        NativeGraphicsApi::WebGpu
    }
}

pub(crate) enum NativeRenderer {
    Vulkan(VulkanRenderer),
    WebGpu(GpuRenderer),
}

impl NativeRenderer {
    pub(crate) fn new_auto(
        display: Option<RawDisplayHandle>,
        window: RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let vulkan_result = VulkanRenderer::new(display, window, width, height);
        match select_native_graphics_api(vulkan_result.is_ok()) {
            NativeGraphicsApi::Vulkan => vulkan_result
                .map(Self::Vulkan)
                .map_err(|error| format!("Не удалось запустить выбранный Vulkan: {error}")),
            NativeGraphicsApi::WebGpu => {
                let vulkan_error = vulkan_result
                    .err()
                    .unwrap_or_else(|| "Vulkan недоступен.".to_owned());
                eprintln!("Vulkan недоступен, выбирается WebGPU: {vulkan_error}");
                GpuRenderer::new_native_webgpu(display, window)
                    .map(Self::WebGpu)
                    .map_err(|webgpu_error| {
                        format!(
                            "Не удалось запустить Vulkan ({vulkan_error}) или WebGPU ({webgpu_error})."
                        )
                    })
            }
        }
    }

    pub(crate) fn renderer_name(&self) -> &str {
        match self {
            Self::Vulkan(renderer) => renderer.renderer_name(),
            Self::WebGpu(renderer) => renderer.renderer_name(),
        }
    }

    pub(crate) fn render(
        &mut self,
        mesh: &SceneMesh,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        match self {
            Self::Vulkan(renderer) => renderer.render(mesh, width, height),
            Self::WebGpu(renderer) => renderer.render(mesh, width, height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeGraphicsApi, select_native_graphics_api};

    #[test]
    fn automatic_selection_prefers_vulkan_when_available() {
        assert_eq!(select_native_graphics_api(true), NativeGraphicsApi::Vulkan);
    }

    #[test]
    fn automatic_selection_uses_webgpu_without_vulkan() {
        assert_eq!(select_native_graphics_api(false), NativeGraphicsApi::WebGpu);
    }
}
