use anyhow::{anyhow, Context};
use serde::Deserialize;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Represents a PipeWire node from pw-dump output
#[derive(Debug, Deserialize)]
struct PwNode {
    id: u32,
    #[serde(default)]
    info: Option<PwNodeInfo>,
}

#[derive(Debug, Deserialize)]
struct PwNodeInfo {
    #[serde(default)]
    props: Option<PwNodeProps>,
}

#[derive(Debug, Deserialize)]
struct PwNodeProps {
    #[serde(rename = "media.class")]
    media_class: Option<String>,
    #[serde(rename = "api.bluez5.address")]
    bluez_address: Option<String>,
    #[serde(rename = "node.name")]
    node_name: Option<String>,
    #[serde(rename = "node.nick")]
    #[allow(dead_code)]
    node_nick: Option<String>,
}

/// Normalize MAC address for comparison (uppercase, colon-separated)
fn normalize_mac(mac: &str) -> String {
    mac.to_uppercase().replace(['-', '_'], ":")
}

/// Find PipeWire sink node ID for a Bluetooth device by MAC address
pub async fn find_sink_by_mac(mac: &str) -> anyhow::Result<Option<(u32, String)>> {
    let normalized = normalize_mac(mac);

    let output = Command::new("pw-dump")
        .output()
        .await
        .context("Failed to run pw-dump")?;

    if !output.status.success() {
        return Err(anyhow!("pw-dump failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let nodes: Vec<PwNode> = serde_json::from_slice(&output.stdout)
        .context("Failed to parse pw-dump JSON")?;

    for node in &nodes {
        let Some(info) = &node.info else { continue };
        let Some(props) = &info.props else { continue };

        let is_sink = props
            .media_class
            .as_deref()
            .map(|c| c == "Audio/Sink")
            .unwrap_or(false);

        let mac_matches = props
            .bluez_address
            .as_deref()
            .map(|a| normalize_mac(a) == normalized)
            .unwrap_or(false);

        if is_sink && mac_matches {
            let name = props.node_name.clone().unwrap_or_default();
            return Ok(Some((node.id, name)));
        }
    }

    Ok(None)
}

/// Set a PipeWire node as the default sink via wpctl
pub async fn set_default_sink(node_id: u32) -> anyhow::Result<()> {
    let output = Command::new("wpctl")
        .args(["set-default", &node_id.to_string()])
        .output()
        .await
        .context("Failed to run wpctl set-default")?;

    if !output.status.success() {
        return Err(anyhow!(
            "wpctl set-default failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    info!("Set default sink to node {node_id}");
    Ok(())
}

/// Migrate all existing sink-inputs to the given sink name via pactl
pub async fn migrate_streams(sink_name: &str) -> anyhow::Result<()> {
    // List current sink-inputs
    let output = Command::new("pactl")
        .args(["list", "short", "sink-inputs"])
        .output()
        .await
        .context("Failed to run pactl list")?;

    if !output.status.success() {
        warn!(
            "pactl list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(()); // Non-fatal
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut migrated = 0;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let input_id = parts[0];

        let move_output = Command::new("pactl")
            .args(["move-sink-input", input_id, sink_name])
            .output()
            .await;

        match move_output {
            Ok(o) if o.status.success() => {
                migrated += 1;
            }
            Ok(o) => {
                debug!(
                    "Failed to move sink-input {input_id}: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            Err(e) => {
                debug!("Failed to run pactl move-sink-input {input_id}: {e}");
            }
        }
    }

    if migrated > 0 {
        info!("Migrated {migrated} audio stream(s) to {sink_name}");
    }

    Ok(())
}

/// Full audio setup after BT device connects:
/// Wait for PipeWire sink to appear, set as default, migrate streams.
pub async fn setup_audio_for_device(mac: &str) -> anyhow::Result<()> {
    let timeout = tokio::time::Duration::from_secs(10);
    let interval = tokio::time::Duration::from_millis(500);
    let start = tokio::time::Instant::now();

    // Retry loop: PipeWire may take a moment to register the BT sink
    let (node_id, node_name) = loop {
        match find_sink_by_mac(mac).await? {
            Some(result) => break result,
            None => {
                if start.elapsed() > timeout {
                    return Err(anyhow!(
                        "Timeout: PipeWire sink for {mac} did not appear within 10s"
                    ));
                }
                debug!("Waiting for PipeWire sink for {mac}...");
                tokio::time::sleep(interval).await;
            }
        }
    };

    info!("Found PipeWire sink for {mac}: node_id={node_id}, name={node_name}");

    // Set as default
    set_default_sink(node_id).await?;

    // Migrate existing streams
    migrate_streams(&node_name).await?;

    Ok(())
}

/// Check if required audio tools are available
pub async fn check_tools() -> Vec<String> {
    let mut missing = Vec::new();

    for tool in &["pw-dump", "wpctl", "pactl"] {
        let result = Command::new("which")
            .arg(tool)
            .output()
            .await;

        match result {
            Ok(o) if o.status.success() => {}
            _ => missing.push(tool.to_string()),
        }
    }

    missing
}
