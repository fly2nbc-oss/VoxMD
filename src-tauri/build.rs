fn main() {
    // When building with `gpu-vulkan`, satisfy whisper-rs-sys's `-lvulkan` /
    // `-lvulkan-1` from our dlopen stub instead of the system shared loader.
    // That keeps libvulkan out of DT_NEEDED so a missing loader does not
    // prevent the process from starting.
    if std::env::var_os("CARGO_FEATURE_GPU_VULKAN").is_some() {
        compile_vulkan_stub();
    }
    tauri_build::build();
}

fn compile_vulkan_stub() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // Match the link name whisper-rs-sys emits so our archive intercepts it.
    let lib_name = if target_os == "windows" {
        "vulkan-1"
    } else {
        "vulkan"
    };

    let mut build = cc::Build::new();
    build.file("vulkan-stub/vulkan_stub.c");
    build.warnings(false);
    if target_os == "windows" {
        build.define("_WIN32", None);
    }
    build.compile(lib_name);

    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    // Ensure the stub directory is searched when resolving `-lvulkan`.
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rerun-if-changed=vulkan-stub/vulkan_stub.c");
}
