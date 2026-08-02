/* Runtime Vulkan loader shim.
 *
 * whisper-rs-sys links `-lvulkan` / `-lvulkan-1`, which normally creates a hard
 * DT_NEEDED on libvulkan.so.1 (or vulkan-1.dll). Missing loader → process fails
 * before main().
 *
 * We compile this file into a static archive named like the system loader
 * (libvulkan.a / vulkan-1.lib) and put its directory first on the link search
 * path. The linker then satisfies Vulkan symbols from this stub; with
 * --as-needed the real shared loader is not recorded as DT_NEEDED.
 *
 * At runtime the stub dlopens the real loader from known absolute paths (so we
 * never reopen ourselves) and forwards. If the loader is absent, probes return
 * safe failure codes so ggml's try/catch in ggml_backend_vk_reg() can fall back.
 */

/* Must precede every libc header to have any effect. */
#if !defined(_WIN32)
#define _GNU_SOURCE
#endif

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <dlfcn.h>
#include <pthread.h>
#endif

typedef void (*PFN_vkVoidFunction)(void);
typedef PFN_vkVoidFunction (*PFN_vkGetInstanceProcAddr)(void *instance, const char *pName);
typedef int32_t VkResult;

/* Match Khronos failure code used when the loader cannot initialise. */
enum { VK_ERROR_INITIALIZATION_FAILED = -3 };

typedef struct VkPhysicalDevice_T *VkPhysicalDevice;
typedef struct VkCommandBuffer_T *VkCommandBuffer;
typedef struct VkBuffer_T *VkBuffer;

typedef struct VkBufferCopy {
    uint64_t srcOffset;
    uint64_t dstOffset;
    uint64_t size;
} VkBufferCopy;

typedef struct VkPhysicalDeviceFeatures2 {
    int32_t sType;
    void *pNext;
    /* Features payload omitted — we only forward or no-op. */
} VkPhysicalDeviceFeatures2;

static void *real_lib;
static PFN_vkGetInstanceProcAddr real_get;

#if defined(_WIN32)
static void *stub_dlsym(void *lib, const char *name) {
    return (void *)GetProcAddress((HMODULE)lib, name);
}
#else
static void *stub_dlsym(void *lib, const char *name) {
    return dlsym(lib, name);
}
#endif

static void load_real_once(void) {
#if defined(_WIN32)
    /*
     * System32 only. The default search order includes the directory of the
     * running executable, and we ship a portable VoxMD.exe that users drop into
     * places like Downloads — a vulkan-1.dll sitting next to it would otherwise
     * be loaded into the process.
     */
    real_lib = (void *)LoadLibraryExA("vulkan-1.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
#else
    /* Absolute paths only — avoid picking up a same-named shim via DT_NEEDED. */
    static const char *const candidates[] = {
        "/usr/lib/libvulkan.so.1",
        "/usr/lib64/libvulkan.so.1",
        "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/usr/lib/aarch64-linux-gnu/libvulkan.so.1",
        "/lib/aarch64-linux-gnu/libvulkan.so.1",
        NULL,
    };
    for (int i = 0; candidates[i] != NULL; i++) {
        real_lib = dlopen(candidates[i], RTLD_NOW | RTLD_LOCAL);
        if (real_lib != NULL) {
            break;
        }
    }
    /* Last resort: normal loader search (safe — we are a static archive, not libvulkan.so.1). */
    if (real_lib == NULL) {
        real_lib = dlopen("libvulkan.so.1", RTLD_NOW | RTLD_LOCAL);
    }
#endif

    if (real_lib != NULL) {
        real_get = (PFN_vkGetInstanceProcAddr)stub_dlsym(real_lib, "vkGetInstanceProcAddr");
    }
}

/*
 * ggml-vulkan calls into these entry points from several threads. The guard used
 * to be a plain `static int`, so two threads could both run the loader and race
 * on `real_lib` / `real_get`.
 */
#if defined(_WIN32)
static INIT_ONCE load_once = INIT_ONCE_STATIC_INIT;

static BOOL CALLBACK load_real_cb(PINIT_ONCE once, PVOID param, PVOID *ctx) {
    (void)once;
    (void)param;
    (void)ctx;
    load_real_once();
    return TRUE;
}

static void load_real(void) {
    InitOnceExecuteOnce(&load_once, load_real_cb, NULL, NULL);
}
#else
static pthread_once_t load_once = PTHREAD_ONCE_INIT;

static void load_real(void) {
    pthread_once(&load_once, load_real_once);
}
#endif

static VkResult stub_enumerate_instance_version(uint32_t *api_version) {
    (void)api_version;
    return VK_ERROR_INITIALIZATION_FAILED;
}

#if defined(_WIN32)
__declspec(dllexport)
#endif
PFN_vkVoidFunction vkGetInstanceProcAddr(void *instance, const char *name) {
    load_real();
    if (real_get != NULL) {
        return real_get(instance, name);
    }
    /* Enough for ggml_vk_instance_init to throw a catchable SystemError. */
    if (name != NULL && strcmp(name, "vkEnumerateInstanceVersion") == 0) {
        return (PFN_vkVoidFunction)stub_enumerate_instance_version;
    }
    return NULL;
}

#if defined(_WIN32)
__declspec(dllexport)
#endif
void vkGetPhysicalDeviceFeatures2(VkPhysicalDevice device, VkPhysicalDeviceFeatures2 *features) {
    typedef void (*Fn)(VkPhysicalDevice, VkPhysicalDeviceFeatures2 *);
    load_real();
    if (real_lib == NULL) {
        return;
    }
    Fn fn = (Fn)stub_dlsym(real_lib, "vkGetPhysicalDeviceFeatures2");
    if (fn != NULL) {
        fn(device, features);
    }
}

#if defined(_WIN32)
__declspec(dllexport)
#endif
void vkCmdCopyBuffer(
    VkCommandBuffer command_buffer,
    VkBuffer src_buffer,
    VkBuffer dst_buffer,
    uint32_t region_count,
    const VkBufferCopy *regions
) {
    typedef void (*Fn)(VkCommandBuffer, VkBuffer, VkBuffer, uint32_t, const VkBufferCopy *);
    load_real();
    if (real_lib == NULL) {
        return;
    }
    Fn fn = (Fn)stub_dlsym(real_lib, "vkCmdCopyBuffer");
    if (fn != NULL) {
        fn(command_buffer, src_buffer, dst_buffer, region_count, regions);
    }
}
