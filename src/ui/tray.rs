use ksni::menu::{StandardItem, MenuItem};
use ksni::TrayMethods;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::ui::window::Command;

/// System tray item using ksni (pure D-Bus SNI, no GTK3)
struct BtSwitchTray {
    cmd_tx: mpsc::UnboundedSender<Command>,
}

impl ksni::Tray for BtSwitchTray {
    fn id(&self) -> String {
        "btswitch".to_string()
    }

    fn title(&self) -> String {
        "Exclusive BT Switcher".to_string()
    }

    fn icon_name(&self) -> String {
        "bluetooth-active".to_string()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left click: toggle window visibility (send Refresh to bring focus)
        let _ = self.cmd_tx.send(Command::ShowWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Show Window".to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.cmd_tx.send(Command::ShowWindow);
                }),
                ..Default::default()
            }
            .into(),
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
/// Returns a handle that keeps the tray alive. Best-effort: if tray fails, app still works.
pub fn spawn_tray(cmd_tx: mpsc::UnboundedSender<Command>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tray tokio runtime");

        rt.block_on(async {
            let tray = BtSwitchTray { cmd_tx };
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
