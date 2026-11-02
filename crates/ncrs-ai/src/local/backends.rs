//! Which compute devices the model may use, and where llama.cpp finds its
//! backend modules.
//!
//! On Windows and x86_64 Linux (`dynamic_backends`, set by build.rs) every
//! backend, the CPU ones too, is a module in a `backends` folder next to the
//! executable. Nothing computes until they are loaded, and a machine without a
//! Vulkan driver only fails to load the Vulkan module: the CPU ones still run.

use llama_cpp_2::LlamaBackendDeviceType;

/// Vulkan lists a software rasteriser as a GPU when no driver is installed.
/// Running the model on it is slower than the CPU backends.
const SOFTWARE_RASTERISERS: [&str; 2] = ["llvmpipe", "lavapipe"];

pub(super) fn is_gpu_type(device_type: LlamaBackendDeviceType) -> bool {
    matches!(
        device_type,
        LlamaBackendDeviceType::Gpu | LlamaBackendDeviceType::IntegratedGpu
    )
}

pub(super) fn is_hardware_gpu(device_type: LlamaBackendDeviceType, description: &str) -> bool {
    let lowercase = description.to_lowercase();
    is_gpu_type(device_type)
        && !SOFTWARE_RASTERISERS
            .iter()
            .any(|software| lowercase.contains(software))
}

pub(super) fn device_type_name(device_type: LlamaBackendDeviceType) -> &'static str {
    match device_type {
        LlamaBackendDeviceType::Cpu => "cpu",
        LlamaBackendDeviceType::Accelerator => "accelerator",
        LlamaBackendDeviceType::Gpu => "gpu",
        LlamaBackendDeviceType::IntegratedGpu => "integrated-gpu",
        LlamaBackendDeviceType::Unknown => "unknown",
    }
}

/// Loads the backend modules; the error is the reason the runtime cannot start.
#[cfg(dynamic_backends)]
pub(super) fn load_modules() -> Result<(), &'static str> {
    let Some(folder) = modules_folder() else {
        return Err("the folder of the compute backends was not found");
    };
    if folder.to_str().is_none() {
        // llama-cpp-2 panics on a path that is not UTF-8.
        return Err("the folder of the compute backends has no usable name");
    }
    llama_cpp_2::llama_backend::load_backends_from_path(&folder);
    if llama_cpp_2::list_llama_ggml_backend_devices().is_empty() {
        return Err("no compute backend was found next to the app");
    }
    Ok(())
}

#[cfg(not(dynamic_backends))]
pub(super) fn load_modules() -> Result<(), &'static str> {
    Ok(())
}

/// Release builds load modules from beside the executable only: the folder
/// the build script knows is a path on the build machine, and loading code
/// from a path on the user's machine that they do not control is how a
/// library gets planted. Debug builds (`cargo run`, `cargo test`) fall back
/// to it.
#[cfg(dynamic_backends)]
fn modules_folder() -> Option<std::path::PathBuf> {
    let beside_executable = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.canonicalize().ok())
        .and_then(|executable| folder_beside(&executable));
    match beside_executable {
        Some(folder) if !cfg!(debug_assertions) || folder.is_dir() => Some(folder),
        _ if cfg!(debug_assertions) => llama_cpp_sys_2::BACKENDS_DIR.map(std::path::PathBuf::from),
        _ => None,
    }
}

#[cfg(dynamic_backends)]
fn folder_beside(executable: &std::path::Path) -> Option<std::path::PathBuf> {
    executable.parent().map(|dir| dir.join("backends"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hardware_gpu_counts() {
        assert!(is_hardware_gpu(
            LlamaBackendDeviceType::Gpu,
            "NVIDIA GeForce RTX 3080"
        ));
        assert!(is_hardware_gpu(
            LlamaBackendDeviceType::IntegratedGpu,
            "Intel(R) UHD Graphics"
        ));
    }

    #[test]
    fn a_software_rasteriser_is_no_gpu_in_any_case() {
        for description in ["llvmpipe (LLVM 17.0.6, 256 bits)", "Lavapipe", "LLVMPIPE"] {
            assert!(!is_hardware_gpu(LlamaBackendDeviceType::Gpu, description));
            assert!(!is_hardware_gpu(
                LlamaBackendDeviceType::IntegratedGpu,
                description
            ));
        }
    }

    #[test]
    fn only_graphics_devices_are_gpus() {
        for device_type in [
            LlamaBackendDeviceType::Cpu,
            LlamaBackendDeviceType::Accelerator,
            LlamaBackendDeviceType::Unknown,
        ] {
            assert!(!is_hardware_gpu(device_type, "NVIDIA GeForce RTX 3080"));
        }
    }

    #[cfg(dynamic_backends)]
    #[test]
    fn the_modules_are_looked_for_beside_the_executable() {
        assert_eq!(
            folder_beside(std::path::Path::new("/usr/lib/ncrs/ncrs")),
            Some(std::path::PathBuf::from("/usr/lib/ncrs/backends"))
        );
    }
}
