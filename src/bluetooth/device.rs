use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    Connected,
    Disconnected,
    Blocked,
    Connecting,
}

impl std::fmt::Display for DeviceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceStatus::Connected => write!(f, "Connected"),
            DeviceStatus::Disconnected => write!(f, "Disconnected"),
            DeviceStatus::Blocked => write!(f, "Blocked"),
            DeviceStatus::Connecting => write!(f, "Connecting"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtAudioDevice {
    /// D-Bus object path, e.g. "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"
    pub path: String,
    /// MAC address, e.g. "AA:BB:CC:DD:EE:FF"
    pub address: String,
    /// Friendly name
    pub alias: String,
    /// Whether the device is paired
    pub paired: bool,
    /// Current status
    pub status: DeviceStatus,
    /// BlueZ UUIDs
    pub uuids: Vec<String>,
    /// BlueZ icon hint (e.g. "audio-headphones")
    pub icon: Option<String>,
}

impl BtAudioDevice {
    /// Derive MAC address from D-Bus object path.
    /// Path format: /org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF
    pub fn mac_from_path(path: &str) -> Option<String> {
        path.rsplit('/')
            .next()
            .and_then(|s| s.strip_prefix("dev_"))
            .map(|s| s.replace('_', ":"))
    }
}
