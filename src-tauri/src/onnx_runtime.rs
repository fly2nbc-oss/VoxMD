//! Lazily provisions the ONNX Runtime shared library used by [`crate::diarize`].
//!
//! `ort` is built with `load-dynamic`, so onnxruntime is *not* linked into the
//! binary — it used to account for 18.7 MB of it, for a feature that is off by
//! default. Instead the library is fetched on first use and dropped next to the
//! two pyannote models, so everything diarization needs lives in one directory.
//!
//! The version is bound to `ort_sys::ORT_API_VERSION`; `version_matches_ort`
//! fails the test suite if an `ort` bump breaks that pairing.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::model_download;

/// ONNX Runtime release matching the ABI `ort` dlopens against.
pub const ONNXRUNTIME_VERSION: &str = "1.22.0";

/// Fails the build, not just the test suite, if an `ort` bump moves the ABI:
/// the pinned release, the asset names and their hashes all have to move with it.
const _: () = assert!(
    ort::MINOR_VERSION == 22,
    "ort now targets a different ONNX Runtime ABI — update ONNXRUNTIME_VERSION,      the release assets and their SHA-256 hashes in onnx_runtime.rs"
);

const BASE: &str = "https://github.com/microsoft/onnxruntime/releases/download";

/// Which variants are constructed depends on the build target.
#[allow(dead_code)]
enum Archive {
    TarGz,
    Zip,
}

struct Dist {
    /// Release asset, without the `{BASE}/v{version}/` prefix.
    asset: &'static str,
    sha256: &'static str,
    archive: Archive,
    /// Path of the library inside the archive.
    member: &'static str,
    /// File name written into the cache directory.
    file_name: &'static str,
}

/// The asset for this build target, or `None` where upstream ships none.
///
/// Hashes are the published release assets for 1.22.0. A mismatch aborts the
/// download rather than caching a corrupt or substituted library.
fn dist() -> Option<Dist> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Some(Dist {
        asset: "onnxruntime-linux-x64-1.22.0.tgz",
        sha256: "8344d55f93d5bc5021ce342db50f62079daf39aaafb5d311a451846228be49b3",
        archive: Archive::TarGz,
        member: "onnxruntime-linux-x64-1.22.0/lib/libonnxruntime.so.1.22.0",
        file_name: "libonnxruntime.so",
    });
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Some(Dist {
        asset: "onnxruntime-linux-aarch64-1.22.0.tgz",
        sha256: "bb76395092d150b52c7092dc6b8f2fe4d80f0f3bf0416d2f269193e347e24702",
        archive: Archive::TarGz,
        member: "onnxruntime-linux-aarch64-1.22.0/lib/libonnxruntime.so.1.22.0",
        file_name: "libonnxruntime.so",
    });
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some(Dist {
        asset: "onnxruntime-win-x64-1.22.0.zip",
        sha256: "174c616efc0271194488642a72f1a514e01487da4dfe84c49296d66e40ebe0da",
        archive: Archive::Zip,
        member: "onnxruntime-win-x64-1.22.0/lib/onnxruntime.dll",
        file_name: "onnxruntime.dll",
    });
    #[cfg(target_os = "macos")]
    return Some(Dist {
        asset: "onnxruntime-osx-universal2-1.22.0.tgz",
        sha256: "cfa6f6584d87555ed9f6e7e8a000d3947554d589efe3723b8bfa358cd263d03c",
        archive: Archive::TarGz,
        member: "onnxruntime-osx-universal2-1.22.0/lib/libonnxruntime.1.22.0.dylib",
        file_name: "libonnxruntime.dylib",
    });
    #[allow(unreachable_code)]
    None
}

fn unsupported() -> String {
    format!(
        "Speaker labels need ONNX Runtime {ONNXRUNTIME_VERSION}, which upstream does not \
         publish for {}-{}.",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// File name of the shared library on this target, or `None` where upstream
/// publishes no build. Used for cache accounting as well as loading.
pub fn library_file_name() -> Option<&'static str> {
    dist().map(|d| d.file_name)
}

/// Where the shared library lives. `dir` is the model directory, so the library
/// sits beside the models that use it.
pub fn library_path(dir: &Path) -> Result<PathBuf, String> {
    let dist = dist().ok_or_else(unsupported)?;
    Ok(dir.join(dist.file_name))
}

pub fn is_cached(dir: &Path) -> bool {
    library_path(dir).is_ok_and(|p| p.is_file())
}

/// Downloads and unpacks the shared library into `dir` unless it is already there.
pub async fn ensure(dir: &Path, on_progress: impl Fn(u64, u64)) -> Result<(), String> {
    let dist = dist().ok_or_else(unsupported)?;
    let dest = dir.join(dist.file_name);
    if dest.is_file() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("Diarize cache: {e}"))?;

    let url = format!("{BASE}/v{ONNXRUNTIME_VERSION}/{}", dist.asset);
    let archive = dir.join(format!("{}.download", dist.asset));
    model_download::download_file(&url, &archive, on_progress).await?;

    let result = verify_and_unpack(&archive, &dist, &dest);
    let _ = std::fs::remove_file(&archive);
    result
}

fn verify_and_unpack(archive: &Path, dist: &Dist, dest: &Path) -> Result<(), String> {
    let bytes = std::fs::read(archive).map_err(|e| format!("Read ONNX Runtime archive: {e}"))?;
    let digest = hex(&Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(dist.sha256) {
        return Err(format!(
            "ONNX Runtime archive failed its checksum (expected {}, got {digest}).",
            dist.sha256
        ));
    }

    // Unpack to a temp name and rename, so an interrupted run cannot leave a
    // truncated library that later looks cached.
    let tmp = dest.with_extension("part");
    let lib = match dist.archive {
        Archive::TarGz => read_tar_member(&bytes, dist.member)?,
        Archive::Zip => read_zip_member(&bytes, dist.member)?,
    };
    std::fs::write(&tmp, lib).map_err(|e| format!("Write ONNX Runtime library: {e}"))?;
    set_executable(&tmp)?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("Install ONNX Runtime library: {e}"))
}

fn read_tar_member(bytes: &[u8], member: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut tar = tar::Archive::new(gz);
    for entry in tar
        .entries()
        .map_err(|e| format!("Read ONNX Runtime archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("Read ONNX Runtime archive: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("Read ONNX Runtime archive: {e}"))?;
        if path.to_string_lossy() != member {
            continue;
        }
        let mut out = Vec::new();
        entry
            .read_to_end(&mut out)
            .map_err(|e| format!("Extract ONNX Runtime library: {e}"))?;
        return Ok(out);
    }
    Err(format!("`{member}` missing from the ONNX Runtime archive."))
}

fn read_zip_member(bytes: &[u8], member: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("Read ONNX Runtime archive: {e}"))?;
    let mut entry = zip
        .by_name(member)
        .map_err(|_| format!("`{member}` missing from the ONNX Runtime archive."))?;
    let mut out = Vec::new();
    entry
        .read_to_end(&mut out)
        .map_err(|e| format!("Extract ONNX Runtime library: {e}"))?;
    Ok(out)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("ONNX Runtime library permissions: {e}"))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

static INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// Points `ort` at the cached library. Must run before the first `Session`.
///
/// `ort` *panics* when its dylib cannot be opened, so the file is checked here
/// and a normal error is returned instead — a failed diarization then degrades
/// to the unlabeled transcript like any other diarization error.
pub fn init(dir: &Path) -> Result<(), String> {
    INIT.get_or_init(|| {
        let path = library_path(dir)?;
        if !path.is_file() {
            return Err(format!(
                "ONNX Runtime is not cached yet ({}). It downloads on first use.",
                path.display()
            ));
        }
        ort::init_from(path.to_string_lossy())
            .commit()
            .map(|_| ())
            .map_err(|e| format!("ONNX Runtime init: {e}"))
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ort` dlopens by ABI version. If an `ort` bump changes `ORT_API_VERSION`,
    /// the pinned release and its hashes have to move with it.
    #[test]
    fn version_matches_ort() {
        // The const assertion above already guards the ABI; this ties the
        // human-readable version string to the same number.
        assert!(ONNXRUNTIME_VERSION.starts_with(&format!("1.{}.", ort::MINOR_VERSION)));
    }

    /// Every asset name and member path has to carry the pinned version, or a
    /// bump would silently keep downloading the old release.
    #[test]
    fn dist_is_self_consistent() {
        let Some(d) = dist() else {
            return; // platform without an upstream build
        };
        assert!(d.asset.contains(ONNXRUNTIME_VERSION), "{}", d.asset);
        assert!(d.member.contains(ONNXRUNTIME_VERSION), "{}", d.member);
        assert!(d.member.contains("/lib/"), "{}", d.member);
        assert_eq!(d.sha256.len(), 64);
        assert!(d.sha256.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// `alternative-backend` is the one ort feature that must never be added
    /// here, and nothing else in the build catches it: the crate compiles, every
    /// unit test passes, CI is green — and then the first `Session` panics with
    /// "attempted to use `ort` APIs before initializing a backend", because the
    /// feature swaps `ort::api()` from `get_or_init(|| dlopen(…))` to a bare
    /// `get()` and `load-dynamic` never runs. Checked against the manifest,
    /// since a dependency's feature set is invisible to `cfg!`.
    #[test]
    fn ort_features_let_load_dynamic_initialise_itself() {
        let manifest = std::fs::read_to_string("Cargo.toml").expect("read Cargo.toml");
        let block = manifest
            .split_once("\nort = {")
            .and_then(|(_, rest)| rest.split_once("] }"))
            .map(|(block, _)| block)
            .expect("ort dependency block in Cargo.toml");

        assert!(
            block.contains("\"load-dynamic\""),
            "ort must keep `load-dynamic`; onnx_runtime.rs provisions the dylib itself"
        );
        assert!(
            !block.contains("alternative-backend"),
            "ort must NOT enable `alternative-backend` — it disables load-dynamic's \
             self-initialisation and every Session panics at runtime"
        );
    }

    #[test]
    fn hex_pads_each_byte() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
