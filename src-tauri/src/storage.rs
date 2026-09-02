use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageRiskCode {
    NetworkLocation,
    CloudSyncLocation,
    RemovableDrive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageWarning {
    pub code: StorageRiskCode,
    pub message: &'static str,
}

pub trait StorageLocationInspector {
    fn inspect(&self, path: &Path) -> Vec<StorageWarning>;
}

#[derive(Debug, Default)]
pub struct SystemStorageLocationInspector;

impl StorageLocationInspector for SystemStorageLocationInspector {
    fn inspect(&self, path: &Path) -> Vec<StorageWarning> {
        let mut warnings = Vec::new();

        if is_network_location(path) {
            warnings.push(StorageWarning {
                code: StorageRiskCode::NetworkLocation,
                message: "该位置可能位于网络磁盘。网络中断或多设备同时写入可能导致数据损坏。",
            });
        }

        if is_cloud_sync_location(path) {
            warnings.push(StorageWarning {
                code: StorageRiskCode::CloudSyncLocation,
                message: "该位置可能由云盘同步。请避免多个设备同时打开仓库，并定期创建完整备份。",
            });
        }

        if is_removable_drive(path) {
            warnings.push(StorageWarning {
                code: StorageRiskCode::RemovableDrive,
                message: "该位置可能位于可移动磁盘。使用期间请勿拔出设备。",
            });
        }

        warnings
    }
}

fn is_cloud_sync_location(path: &Path) -> bool {
    ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .any(|root| starts_with_case_insensitive(path, &root))
        || path
            .components()
            .any(|part| matches_cloud_directory(&part.as_os_str().to_string_lossy()))
}

fn matches_cloud_directory(component: &str) -> bool {
    matches!(
        component.to_ascii_lowercase().as_str(),
        "onedrive" | "dropbox" | "坚果云"
    )
}

fn starts_with_case_insensitive(path: &Path, root: &Path) -> bool {
    let path = path.as_os_str().to_string_lossy().to_ascii_lowercase();
    let root = root.as_os_str().to_string_lossy().to_ascii_lowercase();
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with(['\\', '/']))
}

#[cfg(windows)]
fn is_network_location(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOTE;

    let text = path.as_os_str().to_string_lossy();
    if text.starts_with("\\\\") {
        return true;
    }

    let Some(root) = windows_drive_root(path) else {
        return false;
    };
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a valid null-terminated UTF-16 string for the duration of the call.
    unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_REMOTE }
}

#[cfg(not(windows))]
fn is_network_location(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().starts_with("//")
}

#[cfg(windows)]
fn is_removable_drive(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOVABLE;

    let Some(root) = windows_drive_root(path) else {
        return false;
    };
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a valid null-terminated UTF-16 string for the duration of the call.
    unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_REMOVABLE }
}

#[cfg(not(windows))]
fn is_removable_drive(_path: &Path) -> bool {
    false
}

#[cfg(windows)]
fn windows_drive_root(path: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};

    match path.components().next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                Some(PathBuf::from(format!("{}:\\", char::from(letter))))
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_common_cloud_directory_names() {
        assert!(matches_cloud_directory("OneDrive"));
        assert!(matches_cloud_directory("DROPBOX"));
        assert!(matches_cloud_directory("坚果云"));
        assert!(!matches_cloud_directory("OfferTrackData"));
    }

    #[test]
    fn prefix_comparison_respects_directory_boundary() {
        assert!(starts_with_case_insensitive(
            Path::new(r"C:\Users\Ada\OneDrive\OfferTrack"),
            Path::new(r"c:\users\ada\onedrive")
        ));
        assert!(!starts_with_case_insensitive(
            Path::new(r"C:\Users\Ada\OneDriveElsewhere"),
            Path::new(r"C:\Users\Ada\OneDrive")
        ));
    }
}
