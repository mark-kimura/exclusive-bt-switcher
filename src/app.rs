use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::bluetooth::manager::BtManager;
use crate::state::AppState;
use crate::ui::window::{Command, Event};

/// Run the backend event loop in the Tokio runtime.
/// Receives Commands from the UI, sends Events back via glib channel.
pub async fn run_backend(
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    event_tx: std_mpsc::Sender<Event>,
) {
    let manager = match BtManager::new().await {
        Ok(m) => Arc::new(m),
        Err(e) => {
            error!("Failed to initialize Bluetooth manager: {e}");
            let _ = event_tx.send(Event::Error(format!(
                "Cannot connect to BlueZ: {e}\n\nMake sure Bluetooth service is running."
            )));
            return;
        }
    };

    // Check audio tools
    let missing = crate::audio::pipewire::check_tools().await;
    if !missing.is_empty() {
        warn!("Missing audio tools: {}", missing.join(", "));
        let _ = event_tx.send(Event::Error(format!(
            "Missing required tools: {}. Install PipeWire and WirePlumber.",
            missing.join(", ")
        )));
    }

    // Check adapter power state
    match manager.is_adapter_powered().await {
        Ok(powered) => {
            let _ = event_tx.send(Event::AdapterPowered(powered));
        }
        Err(e) => {
            warn!("Could not check adapter state: {e}");
        }
    }

    // Check for crash recovery
    if let Some(state) = AppState::needs_recovery() {
        info!(
            "Recovery needed: {} blocked device(s)",
            state.app_blocked_devices.len()
        );
        // Send the current device list so UI shows the state
        // The main.rs recovery dialog will handle asking the user
    }

    // Initial device list
    match manager.list_paired_audio_devices().await {
        Ok(devices) => {
            for d in &devices {
                info!("Found device: {} ({}) - {:?}", d.alias, d.address, d.status);
            }
            let _ = event_tx.send(Event::DeviceListUpdated(devices));
        }
        Err(e) => {
            error!("Failed to list devices: {e}");
            let _ = event_tx.send(Event::Error(format!("Failed to list devices: {e}")));
        }
    }

    // Command processing loop
    while let Some(cmd) = cmd_rx.recv().await {
        info!("Received command: {:?}", cmd);
        match cmd {
            Command::SwitchTo(path) => {
                let _ = event_tx.send(Event::SwitchStarted);
                match manager.exclusive_switch(&path).await {
                    Ok(devices) => {
                        let _ = event_tx.send(Event::SwitchComplete);
                        let _ = event_tx.send(Event::DeviceListUpdated(devices));
                    }
                    Err(e) => {
                        error!("Switch failed: {e}");
                        let _ = event_tx.send(Event::Error(format!("{e}")));
                        // Refresh device list to show current state
                        if let Ok(devices) = manager.list_paired_audio_devices().await {
                            let _ = event_tx.send(Event::DeviceListUpdated(devices));
                        }
                    }
                }
            }
            Command::ReleaseAll => {
                let _ = event_tx.send(Event::SwitchStarted);
                match manager.release_all().await {
                    Ok(devices) => {
                        let _ = event_tx.send(Event::SwitchComplete);
                        let _ = event_tx.send(Event::DeviceListUpdated(devices));
                    }
                    Err(e) => {
                        error!("Release all failed: {e}");
                        let _ = event_tx.send(Event::Error(format!("{e}")));
                    }
                }
            }
            Command::Refresh => {
                match manager.is_adapter_powered().await {
                    Ok(powered) => {
                        let _ = event_tx.send(Event::AdapterPowered(powered));
                    }
                    Err(_) => {}
                }
                match manager.list_paired_audio_devices().await {
                    Ok(devices) => {
                        let _ = event_tx.send(Event::DeviceListUpdated(devices));
                    }
                    Err(e) => {
                        let _ = event_tx.send(Event::Error(format!("Refresh failed: {e}")));
                    }
                }
            }
            Command::Shutdown => {
                info!("Shutdown requested, releasing all blocked devices");
                if let Err(e) = manager.release_all().await {
                    warn!("Release on shutdown failed: {e}");
                }
                break;
            }
        }
    }

    info!("Backend loop exited");
    let _ = event_tx.send(Event::Quit);
}
