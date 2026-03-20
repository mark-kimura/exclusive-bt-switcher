use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(Debug, Serialize, Deserialize)]
pub struct AppState {
    /// MAC address of the exclusive target device (if any)
    pub exclusive_target: Option<String>,
    /// MAC addresses of devices blocked by this app
    pub app_blocked_devices: Vec<String>,
    /// Whether a switch operation was in progress (crash recovery)
    pub in_progress: bool,
}

impl AppState {
    fn state_dir() -> PathBuf {
        let dir = dirs_path().join("exclusive-bt-switcher");
        if !dir.exists() {
            let _ = fs::create_dir_all(&dir);
        }
        dir
    }

    fn state_file() -> PathBuf {
        Self::state_dir().join("state.json")
    }

    /// Load state from disk
    pub fn load() -> anyhow::Result<Option<AppState>> {
        let path = Self::state_file();
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)?;
        let state: AppState = serde_json::from_str(&content)?;
        debug!("Loaded state: {:?}", state);
        Ok(Some(state))
    }

    /// Save state indicating an operation is in progress
    pub fn save_in_progress(target_mac: &str, app_blocked: &HashSet<String>) -> anyhow::Result<()> {
        let state = AppState {
            exclusive_target: Some(target_mac.to_string()),
            app_blocked_devices: app_blocked.iter().cloned().collect(),
            in_progress: true,
        };
        Self::atomic_write(&state)
    }

    /// Save clean state (operation completed successfully)
    pub fn save_clean(target_mac: &str, app_blocked: &HashSet<String>) -> anyhow::Result<()> {
        let state = AppState {
            exclusive_target: Some(target_mac.to_string()),
            app_blocked_devices: app_blocked.iter().cloned().collect(),
            in_progress: false,
        };
        Self::atomic_write(&state)
    }

    /// Clear state file (after release all)
    pub fn clear() -> anyhow::Result<()> {
        let path = Self::state_file();
        if path.exists() {
            fs::remove_file(&path)?;
            debug!("State file removed");
        }
        Ok(())
    }

    /// Check if recovery is needed (unclean shutdown with blocked devices)
    pub fn needs_recovery() -> Option<AppState> {
        match Self::load() {
            Ok(Some(state)) if !state.app_blocked_devices.is_empty() => {
                if state.in_progress {
                    warn!("Previous operation was interrupted — recovery needed");
                }
                Some(state)
            }
            _ => None,
        }
    }

    /// Atomic write: write to temp file, then rename
    fn atomic_write(state: &AppState) -> anyhow::Result<()> {
        let path = Self::state_file();
        let tmp = Self::state_dir().join("state.json.tmp");
        let json = serde_json::to_string_pretty(state)?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &path)?;
        debug!("State saved to {:?}", path);
        Ok(())
    }
}

/// Get XDG config home or fallback to ~/.config
fn dirs_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        PathBuf::from("/tmp")
    }
}
