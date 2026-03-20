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
            let address = proxy.address().await.unwrap_or_default();

            let status = if blocked {
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
    /// Returns updated device list on success.
    pub async fn exclusive_switch(
        &self,
        target_path: &str,
    ) -> anyhow::Result<Vec<BtAudioDevice>> {
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

        // 1. Unblock target if blocked
        if target_proxy.blocked().await.unwrap_or(false) {
            info!("Unblocking target device {target_mac}");
            target_proxy.set_blocked(false).await?;
            let mut blocked = self.app_blocked.lock().await;
            blocked.remove(&target_mac);
            // Give BlueZ time to re-register the device after unblocking
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        // 2. Connect target (with retry — devices can be flaky after unblock)
        if !target_proxy.connected().await.unwrap_or(false) {
            let max_retries = 3;
            let mut last_err = None;
            for attempt in 1..=max_retries {
                info!("Connecting to {target_mac} (attempt {attempt}/{max_retries})");
                match target_proxy.connect().await {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        warn!("Connect attempt {attempt} failed: {e}");
                        last_err = Some(e);
                        if attempt < max_retries {
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }
                    }
                }
            }
            if let Some(e) = last_err {
                return Err(anyhow::anyhow!("Failed to connect to {target_mac} after {max_retries} attempts: {e}"));
            }
        }

        // 3. Wait for Connected + ServicesResolved
        self.wait_for_connection(&target_proxy, &target_mac).await?;

        // 4. Wait for PipeWire sink + set default + migrate streams
        crate::audio::pipewire::setup_audio_for_device(&target_mac).await?;

        // 5. Disconnect + block all other paired audio devices
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

            // Disconnect if connected
            if proxy.connected().await.unwrap_or(false) {
                info!("Disconnecting {address}");
                if let Err(e) = proxy.disconnect().await {
                    warn!("Failed to disconnect {address}: {e}");
                }
            }

            // Block to prevent auto-reconnect (only if not already blocked by someone else)
            if !proxy.blocked().await.unwrap_or(false) {
                info!("Blocking {address}");
                if let Err(e) = proxy.set_blocked(true).await {
                    warn!("Failed to block {address}: {e}");
                } else {
                    newly_blocked.push(address.clone());
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

    /// Release all: unblock only devices that THIS app blocked, disconnect target
    pub async fn release_all(&self) -> anyhow::Result<Vec<BtAudioDevice>> {
        let _lock = self.switch_lock.lock().await;
        info!("Releasing all app-blocked devices");

        let mut app_blocked = self.app_blocked.lock().await;
        let addresses_to_unblock: Vec<String> = app_blocked.iter().cloned().collect();

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

            if addresses_to_unblock.contains(&address) {
                if proxy.blocked().await.unwrap_or(false) {
                    info!("Unblocking {address}");
                    if let Err(e) = proxy.set_blocked(false).await {
                        warn!("Failed to unblock {address}: {e}");
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

    /// Wait for Connected=true and ServicesResolved=true (up to 30s)
    async fn wait_for_connection(
        &self,
        proxy: &Device1Proxy<'_>,
        mac: &str,
    ) -> anyhow::Result<()> {
        let timeout = tokio::time::Duration::from_secs(30);
        let start = tokio::time::Instant::now();
        let interval = tokio::time::Duration::from_millis(300);

        loop {
            let connected = proxy.connected().await.unwrap_or(false);
            let resolved = proxy.services_resolved().await.unwrap_or(false);

            if connected && resolved {
                info!("Device {mac} connected and services resolved");
                return Ok(());
            }

            if start.elapsed() > timeout {
                return Err(anyhow::anyhow!(
                    "Timeout waiting for {mac} to connect (connected={connected}, resolved={resolved})"
                ));
            }

            debug!("Waiting for {mac}: connected={connected}, resolved={resolved}");
            tokio::time::sleep(interval).await;
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
