//! 启动器本地设置（保存在 `userData/`，重建 exe 时保留）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const SETTINGS_FILE: &str = "launcher_settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSettings {
    /// 启动时是否自动从远程同步 `resources/`。缺省为 true。
    #[serde(default = "default_true")]
    pub auto_sync_resources: bool,
}

fn default_true() -> bool {
    true
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            auto_sync_resources: true,
        }
    }
}

fn settings_path(user_data_root: &Path) -> std::path::PathBuf {
    user_data_root.join(SETTINGS_FILE)
}

/// 读取启动器设置；缺文件或解析失败时返回默认（自动同步开启）。
pub fn load_launcher_settings(user_data_root: &Path) -> LauncherSettings {
    let path = settings_path(user_data_root);
    if !path.is_file() {
        return LauncherSettings::default();
    }
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => LauncherSettings::default(),
    }
}

/// 写入启动器设置；必要时创建 `userData` 目录。
pub fn save_launcher_settings(user_data_root: &Path, settings: &LauncherSettings) -> Result<(), String> {
    if !user_data_root.is_dir() {
        fs::create_dir_all(user_data_root).map_err(|e| e.to_string())?;
    }
    let path = settings_path(user_data_root);
    let body = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, body).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_auto_sync_on() {
        assert!(LauncherSettings::default().auto_sync_resources);
    }

    #[test]
    fn missing_file_loads_default() {
        let dir = std::env::temp_dir().join(format!(
            "hsr-launcher-settings-missing-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let s = load_launcher_settings(&dir);
        assert!(s.auto_sync_resources);
    }

    #[test]
    fn roundtrip_persists_false() {
        let dir = std::env::temp_dir().join(format!(
            "hsr-launcher-settings-roundtrip-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        save_launcher_settings(
            &dir,
            &LauncherSettings {
                auto_sync_resources: false,
            },
        )
        .unwrap();
        let s = load_launcher_settings(&dir);
        assert!(!s.auto_sync_resources);
        let _ = fs::remove_dir_all(&dir);
    }
}
