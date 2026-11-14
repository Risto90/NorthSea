use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{gpu_renderer::GpuRenderer, scene_mesh::SceneMesh, vulkan_renderer::VulkanRenderer};

const AMD_VENDOR_ID: u32 = 0x1002;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeGraphicsApi {
    Vulkan,
    WebGpu,
}

// AMD относит Radeon RX 400 к низкоуровневым API, RX 500 — к оптимизированным
// для DirectX 12 и Vulkan, а RX Vega — к архитектурам с улучшениями для Vulkan.
// Остальные модели не попадают в эту группу только из-за марки AMD или наличия драйвера.
// Источники AMD:
// https://ir.amd.com/news-events/press-releases/detail/692/amd-provides-sneak-peek-of-full-line-of-radeontm-rx-series-graphics-processors-at-e3
// https://ir.amd.com/news-events/press-releases/detail/763/radeontm-rx-500-series-the-most-compelling-graphics-card-upgrade-yet
// https://ir.amd.com/news-events/press-releases/detail/782/amd-redefines-the-enthusiast-gaming-experience-with-radeon-rx-vega-and-radeon-packs
fn vulkan_priority_model_key(vendor_id: u32, model_name: &str) -> Option<String> {
    if vendor_id != AMD_VENDOR_ID {
        return None;
    }

    let words = model_name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    for model in words.windows(3) {
        if model[0] == "rx" && model[1] == "vega" && matches!(model[2].as_str(), "56" | "64") {
            return Some(format!("rxvega{}", model[2]));
        }
    }
    for model in words.windows(2) {
        if model[0] == "rx"
            && model[1].len() == 3
            && matches!(model[1].as_bytes()[0], b'4' | b'5')
            && model[1].as_bytes()[1..].iter().all(u8::is_ascii_digit)
        {
            return Some(format!("rx{}", model[1]));
        }
    }
    None
}

fn is_vulkan_optimized_gpu(vendor_id: u32, model_name: &str) -> bool {
    vulkan_priority_model_key(vendor_id, model_name).is_some()
}

fn select_native_graphics_api(
    vulkan_optimized_gpu: bool,
    webgpu_available: bool,
    vulkan_available: bool,
) -> Result<NativeGraphicsApi, &'static str> {
    if vulkan_optimized_gpu {
        return vulkan_available
            .then_some(NativeGraphicsApi::Vulkan)
            .ok_or("Для этой модели GPU требуется Vulkan, но он недоступен.");
    }

    if webgpu_available {
        return Ok(NativeGraphicsApi::WebGpu);
    }

    vulkan_available
        .then_some(NativeGraphicsApi::Vulkan)
        .ok_or("Недоступны и WebGPU, и Vulkan.")
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
        match GpuRenderer::new_native_webgpu(display, window) {
            Ok(renderer) => {
                let vendor_id = renderer.adapter_vendor_id();
                let model_name = renderer.adapter_name().to_owned();
                if !is_vulkan_optimized_gpu(vendor_id, &model_name) {
                    return Ok(Self::WebGpu(renderer));
                }
                let model_key =
                    vulkan_priority_model_key(vendor_id, &model_name).ok_or_else(|| {
                        "Не удалось классифицировать модель GPU для Vulkan.".to_owned()
                    })?;

                drop(renderer);
                eprintln!(
                    "Для модели {model_name} из документированного AMD-семейства запускается прямой Vulkan."
                );
                let vulkan_result =
                    VulkanRenderer::new(display, window, width, height, Some(&model_key));
                select_native_graphics_api(true, true, vulkan_result.is_ok()).map_err(|error| {
                    let vulkan_error = vulkan_result
                        .as_ref()
                        .err()
                        .map_or("Vulkan-рендерер не создан", |error| {
                            error.as_str()
                        });
                    format!("{error} Модель: {model_name}. Vulkan: {vulkan_error}")
                })?;
                vulkan_result
                    .map(Self::Vulkan)
                    .map_err(|error| format!("Не удалось запустить прямой Vulkan: {error}"))
            }
            Err(webgpu_error) => {
                eprintln!("WebGPU недоступен, проверяется Vulkan: {webgpu_error}");
                let vulkan_result = VulkanRenderer::new(display, window, width, height, None);
                select_native_graphics_api(false, false, vulkan_result.is_ok()).map_err(
                    |error| {
                        let vulkan_error = vulkan_result
                            .as_ref()
                            .err()
                            .map_or("Vulkan-рендерер не создан", |error| {
                                error.as_str()
                            });
                        format!("{error} WebGPU: {webgpu_error} Vulkan: {vulkan_error}")
                    },
                )?;
                vulkan_result
                    .map(Self::Vulkan)
                    .map_err(|error| format!("Не удалось запустить Vulkan: {error}"))
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
    use super::{
        AMD_VENDOR_ID, NativeGraphicsApi, is_vulkan_optimized_gpu, select_native_graphics_api,
    };

    #[test]
    fn amd_radeon_rx_400_and_rx_500_models_are_vulkan_optimized() {
        for model in [
            "AMD Radeon RX 480 Graphics",
            "AMD Radeon RX 580",
            "Radeon(TM) RX 590 Series",
        ] {
            assert!(is_vulkan_optimized_gpu(AMD_VENDOR_ID, model), "{model}");
        }
    }

    #[test]
    fn amd_radeon_rx_vega_models_are_vulkan_optimized() {
        assert!(is_vulkan_optimized_gpu(
            AMD_VENDOR_ID,
            "AMD Radeon RX Vega 64"
        ));
    }

    #[test]
    fn other_amd_models_and_other_vendors_are_not_vulkan_optimized() {
        assert!(!is_vulkan_optimized_gpu(
            AMD_VENDOR_ID,
            "AMD Radeon RX 5700 XT"
        ));
        assert!(!is_vulkan_optimized_gpu(
            AMD_VENDOR_ID,
            "AMD Radeon RX 7900 XTX"
        ));
        assert!(!is_vulkan_optimized_gpu(0x10de, "NVIDIA Radeon RX 580"));
        assert!(!is_vulkan_optimized_gpu(
            AMD_VENDOR_ID,
            "AMD Radeon Vega 8 Graphics"
        ));
    }

    #[test]
    fn automatic_selection_prefers_direct_vulkan_for_documented_families() {
        assert_eq!(
            select_native_graphics_api(true, true, true),
            Ok(NativeGraphicsApi::Vulkan)
        );
    }

    #[test]
    fn automatic_selection_uses_webgpu_for_other_models_when_available() {
        assert_eq!(
            select_native_graphics_api(false, true, true),
            Ok(NativeGraphicsApi::WebGpu)
        );
        assert_eq!(
            select_native_graphics_api(false, true, false),
            Ok(NativeGraphicsApi::WebGpu)
        );
    }

    #[test]
    fn automatic_selection_uses_vulkan_only_without_webgpu() {
        assert_eq!(
            select_native_graphics_api(false, false, true),
            Ok(NativeGraphicsApi::Vulkan)
        );
    }

    #[test]
    fn automatic_selection_reports_when_required_apis_are_unavailable() {
        assert!(select_native_graphics_api(true, true, false).is_err());
        assert!(select_native_graphics_api(false, false, false).is_err());
    }
}
