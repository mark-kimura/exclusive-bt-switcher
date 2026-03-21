use ksni::menu::{MenuItem, StandardItem};
use ksni::TrayMethods;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::ui::window::Command;

/// 32x32 PNG icon embedded at compile time
const ICON_PNG: &[u8] = include_bytes!("../../resources/icons/btswitch32x32.png");

/// System tray item using ksni (pure D-Bus SNI, no GTK3)
struct BtSwitchTray {
    cmd_tx: mpsc::UnboundedSender<Command>,
    icon_argb: Vec<u8>,
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
            let tray = BtSwitchTray { cmd_tx, icon_argb };
            match tray.spawn().await {
                Ok(_handle) => {
                    info!("System tray icon created");
                    // Keep handle alive forever — dropping it unregisters the tray
                    std::future::pending::<()>().await;
                }
                Err(e) => {
                    warn!("Failed to create system tray: {e}. App works without tray.");
                }
            }
        });
    });
}
