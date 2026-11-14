use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/shaders/scene.wgsl");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" && target_os != "android" {
        return;
    }

    let shader_path =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets/shaders/scene.wgsl");
    let source =
        fs::read_to_string(&shader_path).expect("Не удалось прочитать общий шейдер игровой сцены.");
    let module = naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|error| panic!("Ошибка WGSL: {}", error.emit_to_string(&source)));
    let module_info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("Общий шейдер сцены не прошёл проверку Naga.");

    let output_directory = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    write_spirv(
        &module,
        &module_info,
        naga::ShaderStage::Vertex,
        "vs_main",
        &output_directory.join("scene.vert.spv"),
    );
    write_spirv(
        &module,
        &module_info,
        naga::ShaderStage::Fragment,
        "fs_main",
        &output_directory.join("scene.frag.spv"),
    );
}

fn write_spirv(
    module: &naga::Module,
    module_info: &naga::valid::ModuleInfo,
    shader_stage: naga::ShaderStage,
    entry_point: &str,
    output_path: &std::path::Path,
) {
    let pipeline = naga::back::spv::PipelineOptions {
        shader_stage,
        entry_point: entry_point.to_owned(),
    };
    let mut options = naga::back::spv::Options {
        fake_missing_bindings: false,
        ..Default::default()
    };
    options.binding_map.insert(
        naga::ResourceBinding {
            group: 0,
            binding: 0,
        },
        naga::back::spv::BindingInfo {
            descriptor_set: 0,
            binding: 0,
            binding_array_size: None,
        },
    );
    let words = naga::back::spv::write_vec(module, module_info, &options, Some(&pipeline))
        .unwrap_or_else(|error| panic!("Не удалось преобразовать шейдер Vulkan: {error}"));

    let mut bytes = Vec::with_capacity(words.len() * std::mem::size_of::<u32>());
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    fs::write(output_path, bytes).expect("Не удалось записать SPIR-V шейдер.");
}
