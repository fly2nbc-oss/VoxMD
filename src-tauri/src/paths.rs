//! Where downloaded models live.
//!
//! One directory holds everything the app fetches at runtime: the Whisper GGUF
//! files, the two pyannote ONNX models and the ONNX Runtime library. Filenames
//! do not collide, and keeping them together means one folder to find, move or
//! delete — and one number to report in Settings.
//!
//! It sits in the per-user data directory rather than a cache directory, which
//! is the honest classification: a cache is something the system may evict, and
//! losing a 3 GB model to a cleanup tool would be a nasty surprise. Not the
//! program directory either — an installed build cannot write beside its
//! executable (`C:\Program Files\…`, `/usr/bin` from the `.deb`), and an
//! AppImage's mount is read-only and disappears on exit.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Overrides the location entirely. Set it to keep models on another disk.
pub const MODELS_DIR_ENV: &str = "VOXMD_MODELS_DIR";

/// `%LOCALAPPDATA%\VoxMD` · `~/.local/share/VoxMD` · `~/Library/Application Support/VoxMD`
const APP_DIR: &str = "VoxMD";
const MODELS_SUBDIR: &str = "models";

/// Picks the model directory. Split out from [`models_dir`] so the decision can
/// be tested without touching the environment.
fn resolve(env_override: Option<PathBuf>, data_local: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = env_override {
        if !dir.as_os_str().is_empty() {
            return dir;
        }
    }
    data_local
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
        .join(MODELS_SUBDIR)
}

/// The one directory every downloaded model is written to and loaded from.
///
/// Resolved once per process so a mid-run environment change cannot split
/// writes and reads across two locations.
pub fn models_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        resolve(
            std::env::var_os(MODELS_DIR_ENV).map(PathBuf::from),
            dirs::data_local_dir(),
        )
    })
    .clone()
}

/// Directories 1.0.x used, in the order they should be drained.
fn legacy_dirs() -> Vec<PathBuf> {
    let Some(cache) = dirs::cache_dir() else {
        return Vec::new();
    };
    let voxmd = cache.join("voxmd");
    vec![voxmd.join("whisper"), voxmd.join("diarize")]
}

/// Moves models left behind by 1.0.x into [`models_dir`].
///
/// Called once at startup. Renaming keeps a 3 GB model where it is on the same
/// filesystem; across filesystems the rename fails and the file is copied, so a
/// separate `~/.cache` mount still works. Anything that cannot be moved is left
/// alone and simply downloads again — this is a convenience, never a
/// precondition, so it reports nothing and fails nothing.
pub fn migrate_legacy_models(names: &[String]) -> usize {
    let target = models_dir();
    let mut moved = 0;
    for legacy in legacy_dirs() {
        if !legacy.is_dir() {
            continue;
        }
        for name in names {
            let from = legacy.join(name);
            let to = target.join(name);
            if !from.is_file() || to.exists() {
                continue;
            }
            if std::fs::create_dir_all(&target).is_err() {
                return moved;
            }
            let ok = std::fs::rename(&from, &to).is_ok()
                || (std::fs::copy(&from, &to).is_ok() && std::fs::remove_file(&from).is_ok());
            if ok {
                moved += 1;
            }
        }
        // Only if we emptied it; a directory holding anything else stays.
        let _ = std::fs::remove_dir(&legacy);
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lives_under_the_per_user_data_directory() {
        let data = PathBuf::from("/home/x/.local/share");
        let dir = resolve(None, Some(data.clone()));
        assert_eq!(dir, data.join("VoxMD").join("models"));
    }

    #[test]
    fn the_override_wins_and_is_used_verbatim() {
        let custom = PathBuf::from("/mnt/big-disk/voxmd-models");
        // Not `custom/VoxMD/models`: the user named the directory itself.
        assert_eq!(resolve(Some(custom.clone()), None), custom);
    }

    #[test]
    fn an_empty_override_is_ignored() {
        let data = PathBuf::from("/home/x/.local/share");
        assert_eq!(
            resolve(Some(PathBuf::new()), Some(data.clone())),
            data.join("VoxMD").join("models")
        );
    }

    /// `dirs` returns `None` on an exotic platform; a relative path still works
    /// and beats refusing to run.
    #[test]
    fn falls_back_to_a_relative_path_without_a_data_directory() {
        assert_eq!(
            resolve(None, None),
            PathBuf::from(".").join("VoxMD").join("models")
        );
    }

    #[test]
    fn migration_moves_only_known_files_and_never_overwrites() {
        let root = std::env::temp_dir().join(format!("voxmd-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join("legacy");
        let target = root.join("target");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&target).unwrap();

        std::fs::write(legacy.join("ggml-small.bin"), b"model").unwrap();
        std::fs::write(legacy.join("notes.txt"), b"not ours").unwrap();
        std::fs::write(legacy.join("kept.bin"), b"old").unwrap();
        std::fs::write(target.join("kept.bin"), b"new").unwrap();

        let names = ["ggml-small.bin".to_string(), "kept.bin".to_string()];
        let mut moved = 0;
        for name in &names {
            let from = legacy.join(name);
            let to = target.join(name);
            if !from.is_file() || to.exists() {
                continue;
            }
            if std::fs::rename(&from, &to).is_ok() {
                moved += 1;
            }
        }

        assert_eq!(moved, 1);
        assert_eq!(
            std::fs::read(target.join("ggml-small.bin")).unwrap(),
            b"model"
        );
        // An existing target is never clobbered, and a stranger is never touched.
        assert_eq!(std::fs::read(target.join("kept.bin")).unwrap(), b"new");
        assert!(legacy.join("notes.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_real_directory_is_absolute_and_stable() {
        let dir = models_dir();
        assert!(dir.is_absolute(), "{dir:?}");
        assert!(
            dir.ends_with("VoxMD/models") || dir.ends_with("VoxMD\\models"),
            "{dir:?}"
        );
        assert_eq!(models_dir(), dir);
    }
}
