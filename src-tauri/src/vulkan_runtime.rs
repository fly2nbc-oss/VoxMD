//! Runtime Vulkan loader probe (independent of compile-time `gpu-vulkan`).

/// True when this binary was built with the `gpu-vulkan` Cargo feature.
pub fn built_with_vulkan() -> bool {
    cfg!(feature = "gpu-vulkan")
}

/// True when the system Vulkan loader library can be opened.
///
/// On `gpu-vulkan` builds the process no longer hard-depends on the loader at
/// startup; GPU use still requires it at runtime.
pub fn loader_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_loader_available()
    }
    #[cfg(not(target_os = "windows"))]
    {
        unix_loader_available()
    }
}

/// GPU path is usable: built with Vulkan support and loader present.
pub fn gpu_usable() -> bool {
    built_with_vulkan() && loader_available()
}

#[cfg(target_os = "windows")]
fn windows_loader_available() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
        fn FreeLibrary(module: *mut core::ffi::c_void) -> i32;
    }

    let wide: Vec<u16> = OsStr::new("vulkan-1.dll")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let handle = LoadLibraryW(wide.as_ptr());
        if handle.is_null() {
            return false;
        }
        FreeLibrary(handle);
        true
    }
}

#[cfg(not(target_os = "windows"))]
fn unix_loader_available() -> bool {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_void};

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    const RTLD_NOW: c_int = 0x2;

    // Same absolute candidates as vulkan-stub/vulkan_stub.c (avoid relative names).
    const CANDIDATES: &[&str] = &[
        "/usr/lib/libvulkan.so.1",
        "/usr/lib64/libvulkan.so.1",
        "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/usr/lib/aarch64-linux-gnu/libvulkan.so.1",
        "/lib/aarch64-linux-gnu/libvulkan.so.1",
    ];

    for path in CANDIDATES {
        let Ok(c) = CString::new(*path) else {
            continue;
        };
        unsafe {
            let handle = dlopen(c.as_ptr(), RTLD_NOW);
            if !handle.is_null() {
                dlclose(handle);
                return true;
            }
        }
    }

    // Same last-resort search as the link stub (Nix, Homebrew, custom rpaths).
    if let Ok(c) = CString::new("libvulkan.so.1") {
        unsafe {
            let handle = dlopen(c.as_ptr(), RTLD_NOW);
            if !handle.is_null() {
                dlclose(handle);
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_usable_implies_built() {
        if gpu_usable() {
            assert!(built_with_vulkan());
            assert!(loader_available());
        }
    }
}
