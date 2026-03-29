use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for key in [
        "CUDA_ROOT",
        "CUDA_PATH",
        "CUDA_HOME",
        "CUDAToolkit_ROOT",
        "CUDA_TOOLKIT_ROOT_DIR",
        "HOME",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }

    if env::var_os("CARGO_FEATURE_CUDA").is_none() {
        return;
    }

    let Some(cuda_root) = find_cuda_root() else {
        println!("cargo:warning=CUDA toolkit root not found; relying on ambient linker paths");
        return;
    };

    let cuda_lib = cuda_root.join("lib");
    let cuda_stubs = cuda_lib.join("stubs");
    let driver_lib = Path::new("/run/opengl-driver/lib");

    println!("cargo:rustc-env=BIT_SCAN_CUDA_ROOT={}", cuda_root.display());

    if driver_lib.join("libcuda.so").exists() {
        println!("cargo:rustc-link-search=native={}", driver_lib.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", driver_lib.display());
    }
    if cuda_lib.join("libnvrtc.so").exists() {
        println!("cargo:rustc-link-search=native={}", cuda_lib.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", cuda_lib.display());
    }
    if cuda_stubs.join("libcuda.so").exists() {
        println!("cargo:rustc-link-search=native={}", cuda_stubs.display());
    }
}

fn find_cuda_root() -> Option<PathBuf> {
    for key in [
        "CUDA_ROOT",
        "CUDA_PATH",
        "CUDA_HOME",
        "CUDAToolkit_ROOT",
        "CUDA_TOOLKIT_ROOT_DIR",
    ] {
        if let Some(path) = env::var_os(key).map(PathBuf::from)
            && has_nvrtc(&path)
        {
            return Some(path);
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let profile = PathBuf::from(home).join(".nix-profile");
        if has_nvrtc(&profile) {
            return Some(profile);
        }
    }

    [
        PathBuf::from("/run/current-system/sw"),
        PathBuf::from("/usr/local/cuda"),
        PathBuf::from("/opt/cuda"),
    ]
    .into_iter()
    .find(|path| has_nvrtc(path))
}

fn has_nvrtc(root: &Path) -> bool {
    root.join("lib/libnvrtc.so").exists()
}
