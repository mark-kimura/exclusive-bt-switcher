use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};
use ksni::TrayMethods;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::ui::window::Command;

/// 32x32 PNG icon embedded at compile time
const ICON_PNG: &[u8] = include_bytes!("../../resources/icons/btswitch32x32.png");

/// Build autostart desktop entry with the full path to the current binary
fn autostart_entry() -> String {
    let exe = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("btswitch"));
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Exclusive BT Switcher\n\
         Exec={} --minimized\n\
         StartupNotify=false\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exe.display()
    )
}

/// System tray item using ksni (pure D-Bus SNI, no GTK3)
struct BtSwitchTray {
    cmd_tx: mpsc::UnboundedSender<Command>,
    icon_argb: Vec<u8>,
    autostart: bool,
}

fn autostart_path() -> PathBuf {
    let dir = dirs_path().join("autostart");
    std::fs::create_dir_all(&dir).ok();
    dir.join("btswitch.desktop")
}

fn dirs_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        PathBuf::from("/tmp")
    }
}

fn is_autostart_enabled() -> bool {
    autostart_path().exists()
}

fn set_autostart(enabled: bool) {
    let path = autostart_path();
    if enabled {
        if let Err(e) = std::fs::write(&path, autostart_entry()) {
            warn!("Failed to create autostart entry: {e}");
        } else {
            info!("Autostart enabled");
        }
    } else {
        if let Err(e) = std::fs::remove_file(&path) {
            warn!("Failed to remove autostart entry: {e}");
        } else {
            info!("Autostart disabled");
        }
    }
}

/// Decode PNG bytes into ARGB pixel data (big-endian, as ksni expects)
fn decode_png_to_argb(png_data: &[u8]) -> (i32, i32, Vec<u8>) {
    let decoder = png::Decoder::new(png_data);
    let mut reader = decoder.read_info().expect("valid PNG");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("PNG frame");
    let width = info.width as i32;
    let height = info.height as i32;

    // Convert RGBA to ARGB (big-endian u32)
    let mut argb = Vec::with_capacity((width * height * 4) as usize);
    for chunk in buf[..info.buffer_size()].chunks(4) {
        let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        argb.extend_from_slice(&[a, r, g, b]);
    }
    (width, height, argb)
}

impl ksni::Tray for BtSwitchTray {
    fn id(&self) -> String {
        "btswitch".to_string()
    }

    fn title(&self) -> String {
        "Exclusive BT Switcher".to_string()
    }

    fn icon_name(&self) -> String {
        String::new() // Use pixmap instead
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon {
            width: 32,
            height: 32,
            data: self.icon_argb.clone(),
        }]
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.cmd_tx.send(Command::ShowWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Unblock All".to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.cmd_tx.send(Command::ReleaseAll);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Start at Login".to_string(),
                checked: self.autostart,
                activate: Box::new(|tray: &mut Self| {
                    tray.autostart = !tray.autostart;
                    set_autostart(tray.autostart);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.cmd_tx.send(Command::Shutdown);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Spawn the system tray in a background task.
/// Best-effort: if tray fails, app still works.
pub fn spawn_tray(cmd_tx: mpsc::UnboundedSender<Command>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tray tokio runtime");

        rt.block_on(async {
            let (_w, _h, icon_argb) = decode_png_to_argb(ICON_PNG);

            // Retry tray creation — at login, the StatusNotifierWatcher
            // may not be ready yet when the app autostarts
            let max_attempts = 10;
            for attempt in 1..=max_attempts {
                let tray = BtSwitchTray {
                    cmd_tx: cmd_tx.clone(),
                    icon_argb: icon_argb.clone(),
                    autostart: is_autostart_enabled(),
                };
                match tray.spawn().await {
                    Ok(_handle) => {
                        info!("System tray icon created (attempt {attempt})");
                        // Keep handle alive forever — dropping it unregisters the tray
                        std::future::pending::<()>().await;
                    }
                    Err(e) => {
                        if attempt < max_attempts {
                            warn!("Tray attempt {attempt} failed: {e}. Retrying in 3s...");
                            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        } else {
                            warn!("Failed to create system tray after {max_attempts} attempts: {e}. App works without tray.");
                        }
                    }
                }
            }
        });
    });
}
