/// A2DP Sink — Advanced Audio Distribution Profile
pub const A2DP_SINK: &str = "0000110b-0000-1000-8000-00805f9b34fb";

/// A2DP Source
pub const A2DP_SOURCE: &str = "0000110a-0000-1000-8000-00805f9b34fb";

/// HFP Hands-Free
pub const HFP_HF: &str = "0000111e-0000-1000-8000-00805f9b34fb";

/// HFP Audio Gateway
pub const HFP_AG: &str = "0000111f-0000-1000-8000-00805f9b34fb";

/// HSP Headset
pub const HSP_HS: &str = "00001108-0000-1000-8000-00805f9b34fb";

/// HSP Audio Gateway
pub const HSP_AG: &str = "00001112-0000-1000-8000-00805f9b34fb";

/// LE Audio — Basic Audio Profile
pub const LE_AUDIO_BAP: &str = "00001850-0000-1000-8000-00805f9b34fb";

/// LE Audio — Media Control
pub const LE_AUDIO_MCS: &str = "00001848-0000-1000-8000-00805f9b34fb";

/// LE Audio — Common Audio Profile
pub const LE_AUDIO_CAP: &str = "0000184e-0000-1000-8000-00805f9b34fb";

const AUDIO_UUIDS: &[&str] = &[
    A2DP_SINK,
    A2DP_SOURCE,
    HFP_HF,
    HFP_AG,
    HSP_HS,
    HSP_AG,
    LE_AUDIO_BAP,
    LE_AUDIO_MCS,
    LE_AUDIO_CAP,
];

/// Returns true if any of the device's UUIDs indicate it's an audio device.
pub fn is_audio_device(uuids: &[String]) -> bool {
    uuids
        .iter()
        .any(|uuid| AUDIO_UUIDS.contains(&uuid.to_lowercase().as_str()))
}
