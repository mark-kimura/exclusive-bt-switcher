use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use zbus::Connection;

use crate::bluetooth::device::{BtAudioDevice, DeviceStatus};
use crate::bluetooth::uuids;
use crate::state::AppState;

/// zbus proxy for BlueZ org.bluez.Device1 interface
#[zbus::proxy(
    interface = "org.bluez.Device1",
    default_service = "org.bluez"
)]
trait Device1 {
    fn connect(&self) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn address(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn alias(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn paired(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn connected(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn blocked(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_blocked(&self, blocked: bool) -> zbus::Result<()>;

    #[zbus(property, name = "UUIDs")]
    fn uuids(&self) -> zbus::Result<Vec<String>>;

    #[zbus(property)]
    fn icon(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn services_resolved(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn trusted(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_trusted(&self, trusted: bool) -> zbus::Result<()>;

    #[zbus(property)]
    fn address_type(&self) -> zbus::Result<String>;
}

/// zbus proxy for BlueZ org.bluez.Adapter1 interface
#[zbus::proxy(
    interface = "org.bluez.Adapter1",
    default_service = "org.bluez"
)]
trait Adapter1 {
    #[zbus(property)]
    fn powered(&self) -> zbus::Result<bool>;
}

/// zbus proxy for ObjectManager to enumerate BlueZ objects
#[zbus::proxy(
    interface = "org.freedesktop.DBus.ObjectManager",
    default_service = "org.bluez",
    default_path = "/"
)]
trait ObjectManager {
    fn get_managed_objects(
        &self,
    ) -> zbus::Result<
        HashMap<
            zbus::zvariant::OwnedObjectPath,
            HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>,
        >,
    >;
}

pub struct BtManager {
    connection: Connection,
    /// Devices that THIS app instance has blocked (by address)
    app_blocked: Arc<Mutex<HashSet<String>>>,
    /// Lock to serialize switch operations
    switch_lock: Arc<Mutex<()>>,
}

impl BtManager {
    pub async fn new() -> anyhow::Result<Self> {
        let connection = Connection::system().await?;
        let state = AppState::load()?;
        let app_blocked: HashSet<String> = state
            .map(|s| s.app_blocked_devices.into_iter().collect())
            .unwrap_or_default();

        Ok(Self {
            connection,
            app_blocked: Arc::new(Mutex::new(app_blocked)),
            switch_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Check if the BT adapter is powered on
    pub async fn is_adapter_powered(&self) -> anyhow::Result<bool> {
        let objects = self.get_managed_objects().await?;
        for (path, interfaces) in &objects {
            if interfaces.contains_key("org.bluez.Adapter1") {
                let proxy = Adapter1Proxy::builder(&self.connection)
                    .path(path.as_ref())?
                    .build()
                    .await?;
                return Ok(proxy.powered().await.unwrap_or(false));
            }
        }
        Ok(false)
    }

    /// List all paired Bluetooth audio devices
    pub async fn list_paired_audio_devices(&self) -> anyhow::Result<Vec<BtAudioDevice>> {
        let objects = self.get_managed_objects().await?;
        let app_blocked = self.app_blocked.lock().await;
        let mut devices = Vec::new();

        for (path, interfaces) in &objects {
            if !interfaces.contains_key("org.bluez.Device1") {
                continue;
            }

            let path_str = path.as_str();
            let proxy = Device1Proxy::builder(&self.connection)
                .path(path_str)?
                .build()
                .await?;

            let paired = proxy.paired().await.unwrap_or(false);
            if !paired {
                continue;
            }

            let device_uuids = proxy.uuids().await.unwrap_or_default();
            if !uuids::is_audio_device(&device_uuids) {
                continue;
            }

            let connected = proxy.connected().await.unwrap_or(false);
            let blocked = proxy.blocked().await.unwrap_or(false);
            let trusted = proxy.trusted().await.unwrap_or(true);
            let address = proxy.address().await.unwrap_or_default();
            let is_le = Self::is_le_device(&proxy).await;

            // A device is "blocked" if:
            // - Classic BT: Blocked=true (kernel-level block)
            // - BLE: Trusted=false and this app suppressed it (soft block)
            let app_suppressed = {
                let ab = app_blocked.clone();
                ab.contains(&address)
            };
            let status = if blocked {
                DeviceStatus::Blocked
            } else if is_le && !trusted && app_suppressed {
                DeviceStatus::Blocked
            } else if connected {
                DeviceStatus::Connected
            } else {
                DeviceStatus::Disconnected
            };

            devices.push(BtAudioDevice {
                path: path_str.to_string(),
                address: address.clone(),
                alias: proxy.alias().await.unwrap_or_else(|_| address.clone()),
                paired,
                status,
                uuids: device_uuids,
                icon: proxy.icon().await.ok(),
            });
        }

        // Sort by alias only — stable order regardless of status changes
        devices.sort_by(|a, b| a.alias.cmp(&b.alias));

        drop(app_blocked);
        Ok(devices)
    }

    /// Execute exclusive switch: connect target, block all others.
    /// Calls `on_progress` with a status message at each step.
    /// Returns updated device list on success.
    pub async fn exclusive_switch<F>(
        &self,
        target_path: &str,
        on_progress: F,
    ) -> anyhow::Result<Vec<BtAudioDevice>>
    where
        F: Fn(&str),
    {
        let _lock = self.switch_lock.lock().await;
        info!("Starting exclusive switch to {target_path}");

        let target_mac = BtAudioDevice::mac_from_path(target_path)
            .ok_or_else(|| anyhow::anyhow!("Invalid device path: {target_path}"))?;

        // Mark operation in progress
        {
            let blocked = self.app_blocked.lock().await;
            AppState::save_in_progress(&target_mac, &blocked)?;
        }

        let target_proxy = Device1Proxy::builder(&self.connection)
            .path(target_path)?
            .build()
            .await?;

        let target_alias = target_proxy.alias().await.unwrap_or_else(|_| target_mac.clone());

        let target_is_le = Self::is_le_device(&target_proxy).await;
        let was_suppressed = {
            let ab = self.app_blocked.lock().await;
            ab.contains(&target_mac)
        };

        // 1. Enable target device (undo suppression)
        if was_suppressed {
            if target_is_le {
                // BLE: restore Trusted=true (the Connect() call later will re-enable incoming)
                info!("Restoring trust for BLE device {target_mac}");
                target_proxy.set_trusted(true).await?;
            } else {
                // Classic: unblock
                info!("Unblocking classic device {target_mac}");
                target_proxy.set_blocked(false).await?;
                // Give BlueZ time to re-register after unblocking
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
            {
                let mut blocked = self.app_blocked.lock().await;
                blocked.remove(&target_mac);
            }
            on_progress(&format!("Connecting to {target_alias}..."));
        }

        // Steps 2-4: connect, wait for audio sink, setup audio.
        // If any step fails, re-suppress the target.
        if let Err(e) = self.connect_and_setup(&target_proxy, &target_mac, &target_alias, target_is_le).await {
            if was_suppressed {
                warn!("Re-suppressing {target_mac} after failed switch");
                if target_is_le {
                    let _ = target_proxy.set_trusted(false).await;
                    let _ = target_proxy.disconnect().await;
                } else {
                    let _ = target_proxy.set_blocked(true).await;
                }
                let mut blocked = self.app_blocked.lock().await;
                blocked.insert(target_mac.clone());
            }
            // Disconnect if partially connected
            if target_proxy.connected().await.unwrap_or(false) {
                let _ = target_proxy.disconnect().await;
            }
            return Err(e);
        }

        on_progress("Blocking other devices...");

        // 5. Suppress all other paired audio devices
        // Hybrid approach: Blocked=true for classic BT, Trusted=false+Disconnect for BLE
        let all_devices = self.list_paired_audio_devices_raw().await?;
        let mut newly_blocked = Vec::new();
        for (path, address) in &all_devices {
            if address == &target_mac {
                continue;
            }
            let proxy = Device1Proxy::builder(&self.connection)
                .path(path.as_str())?
                .build()
                .await?;

            let is_le = Self::is_le_device(&proxy).await;

            if is_le {
                // BLE: use Trusted=false + Disconnect() to suppress
                // This avoids GATT cache corruption that Blocked=true causes
                info!("Suppressing BLE device {address} (Trusted=false + Disconnect)");
                if let Err(e) = proxy.set_trusted(false).await {
                    warn!("Failed to untrust {address}: {e}");
                }
                if proxy.connected().await.unwrap_or(false) {
                    if let Err(e) = proxy.disconnect().await {
                        warn!("Failed to disconnect {address}: {e}");
                    }
                }
                newly_blocked.push(address.clone());
            } else {
                // Classic BT: use Blocked=true (reliable, no issues)
                if proxy.connected().await.unwrap_or(false) {
                    info!("Disconnecting {address}");
                    if let Err(e) = proxy.disconnect().await {
                        warn!("Failed to disconnect {address}: {e}");
                    }
                }
                if !proxy.blocked().await.unwrap_or(false) {
                    info!("Blocking classic device {address}");
                    if let Err(e) = proxy.set_blocked(true).await {
                        warn!("Failed to block {address}: {e}");
                    } else {
                        newly_blocked.push(address.clone());
                    }
                }
            }
        }

        // 6. Update app-blocked tracking and persist clean state
        {
            let mut app_blocked = self.app_blocked.lock().await;
            for addr in newly_blocked {
                app_blocked.insert(addr);
            }
            AppState::save_clean(&target_mac, &app_blocked)?;
        }

        info!("Exclusive switch to {target_mac} complete");
        self.list_paired_audio_devices().await
    }

    /// Release all: undo suppression on devices that THIS app suppressed
    pub async fn release_all(&self) -> anyhow::Result<Vec<BtAudioDevice>> {
        let _lock = self.switch_lock.lock().await;
        info!("Releasing all app-suppressed devices");

        let mut app_blocked = self.app_blocked.lock().await;
        let addresses_to_release: Vec<String> = app_blocked.iter().cloned().collect();

        let objects = self.get_managed_objects().await?;

        for (path, interfaces) in &objects {
            if !interfaces.contains_key("org.bluez.Device1") {
                continue;
            }
            let proxy = Device1Proxy::builder(&self.connection)
                .path(path.as_ref())?
                .build()
                .await?;

            let address = proxy.address().await.unwrap_or_default();

            if addresses_to_release.contains(&address) {
                let is_le = Self::is_le_device(&proxy).await;

                if is_le {
                    // BLE: restore Trusted=true
                    info!("Restoring trust for BLE device {address}");
                    if let Err(e) = proxy.set_trusted(true).await {
                        warn!("Failed to re-trust {address}: {e}");
                    }
                } else {
                    // Classic: unblock
                    if proxy.blocked().await.unwrap_or(false) {
                        info!("Unblocking classic device {address}");
                        if let Err(e) = proxy.set_blocked(false).await {
                            warn!("Failed to unblock {address}: {e}");
                        }
                    }
                }
            }
        }

        app_blocked.clear();
        AppState::clear()?;
        drop(app_blocked);

        info!("Release all complete");
        self.list_paired_audio_devices().await
    }

    /// Connect target, wait for readiness, setup audio. Returns error on failure.
    /// For BLE devices, skips ServicesResolved and uses PipeWire sink as readiness signal.
    async fn connect_and_setup(
        &self,
        proxy: &Device1Proxy<'_>,
        mac: &str,
        alias: &str,
        is_le: bool,
    ) -> anyhow::Result<()> {
        // For BLE: wait briefly to see if device auto-connects before calling Connect()
        if is_le && !proxy.connected().await.unwrap_or(false) {
            info!("Waiting for BLE device {mac} to auto-connect...");
            let auto_wait = tokio::time::Duration::from_secs(4);
            let start = tokio::time::Instant::now();
            while start.elapsed() < auto_wait {
                if proxy.connected().await.unwrap_or(false) {
                    info!("BLE device {mac} auto-connected");
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }
        }

        // Connect with retry (if not already connected)
        if !proxy.connected().await.unwrap_or(false) {
            let max_retries = 3;
            let mut last_err = None;
            for attempt in 1..=max_retries {
                info!("Connecting to {mac} (attempt {attempt}/{max_retries})");
                match proxy.connect().await {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        warn!("Connect attempt {attempt} failed: {e}");
                        if err_str.contains("InProgress") || err_str.contains("busy") {
                            info!("Connection already in progress, waiting...");
                            last_err = None;
                            break;
                        }
                        last_err = Some(e);
                        if attempt < max_retries {
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }
                    }
                }
            }
            if let Some(e) = last_err {
                let err_str = e.to_string();
                let friendly = if err_str.contains("NotReady") || err_str.contains("not available") {
                    format!("{alias} is not powered on or out of range.")
                } else if err_str.contains("connection-unknown") || err_str.contains("Does Not Exist") {
                    format!("{alias} could not be reached.")
                } else if err_str.contains("ConnectFailed") || err_str.contains("Page Timeout") || err_str.contains("page-timeout") {
                    format!("{alias} is not responding.")
                } else {
                    format!("{alias} could not be connected.")
                };
                return Err(anyhow::anyhow!(friendly));
            }
        }

        if is_le {
            // BLE: skip ServicesResolved — go straight to waiting for PipeWire sink
            // Wait for Connected=true first
            let timeout = tokio::time::Duration::from_secs(10);
            let start = tokio::time::Instant::now();
            while start.elapsed() < timeout {
                if proxy.connected().await.unwrap_or(false) {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }
            if !proxy.connected().await.unwrap_or(false) {
                return Err(anyhow::anyhow!("{alias} is not responding."));
            }
            info!("BLE device {mac} connected, waiting for PipeWire sink...");
        } else {
            // Classic BT: wait for ServicesResolved as before
            self.wait_for_services(proxy, mac, alias).await?;
        }

        // Setup audio — PipeWire sink appearance is the final readiness signal
        crate::audio::pipewire::setup_audio_for_device(mac).await?;

        Ok(())
    }

    /// Wait for Connected=true and ServicesResolved=true (classic BT only).
    /// If services don't resolve within 10s, disconnect and reconnect once.
    async fn wait_for_services(
        &self,
        proxy: &Device1Proxy<'_>,
        mac: &str,
        alias: &str,
    ) -> anyhow::Result<()> {
        let interval = tokio::time::Duration::from_millis(300);

        for attempt in 1..=2 {
            let attempt_timeout = if attempt == 1 { 10 } else { 15 };
            let start = tokio::time::Instant::now();

            loop {
                let connected = proxy.connected().await.unwrap_or(false);
                let resolved = proxy.services_resolved().await.unwrap_or(false);

                if connected && resolved {
                    info!("Device {mac} connected and services resolved");
                    return Ok(());
                }

                if start.elapsed() > tokio::time::Duration::from_secs(attempt_timeout) {
                    if !connected {
                        return Err(anyhow::anyhow!(
                            "{alias} is not responding."
                        ));
                    }
                    // Connected but services not resolved — try disconnect+reconnect
                    if attempt == 1 {
                        warn!("Services not resolved for {mac}, reconnecting...");
                        let _ = proxy.disconnect().await;
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        let _ = proxy.connect().await;
                        break; // go to attempt 2
                    } else {
                        return Err(anyhow::anyhow!(
                            "{alias} connected but audio services did not start. Try again."
                        ));
                    }
                }

                debug!("Waiting for {mac}: connected={connected}, resolved={resolved}");
                tokio::time::sleep(interval).await;
            }
        }
        unreachable!()
    }

    /// Determine if a device uses LE (BLE) transport.
    /// BlueZ sets AddressType to "random" for BLE devices with random addresses.
    /// For BR/EDR, AddressType is typically "public" or absent.
    /// We use "random" as the definitive BLE indicator.
    async fn is_le_device(proxy: &Device1Proxy<'_>) -> bool {
        match proxy.address_type().await {
            Ok(addr_type) => addr_type == "random",
            Err(_) => false,
        }
    }

    /// Get raw list of paired audio device paths + MAC addresses
    async fn list_paired_audio_devices_raw(&self) -> anyhow::Result<Vec<(zbus::zvariant::OwnedObjectPath, String)>> {
        let objects = self.get_managed_objects().await?;
        let mut result = Vec::new();

        for (path, interfaces) in &objects {
            if !interfaces.contains_key("org.bluez.Device1") {
                continue;
            }
            let proxy = Device1Proxy::builder(&self.connection)
                .path(path.as_ref())?
                .build()
                .await?;

            if !proxy.paired().await.unwrap_or(false) {
                continue;
            }

            let device_uuids = proxy.uuids().await.unwrap_or_default();
            if !uuids::is_audio_device(&device_uuids) {
                continue;
            }

            let address = proxy.address().await.unwrap_or_default();
            result.push((path.clone(), address));
        }

        Ok(result)
    }

    async fn get_managed_objects(
        &self,
    ) -> anyhow::Result<
        HashMap<
            zbus::zvariant::OwnedObjectPath,
            HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>,
        >,
    > {
        let proxy = ObjectManagerProxy::new(&self.connection).await?;
        Ok(proxy.get_managed_objects().await?)
    }

    /// Get addresses this app has blocked (for recovery UI)
    #[allow(dead_code)]
    pub async fn get_app_blocked_addresses(&self) -> Vec<String> {
        self.app_blocked.lock().await.iter().cloned().collect()
    }
}
