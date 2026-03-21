mod app;
mod audio;
mod bluetooth;
mod error;
mod state;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc as std_mpsc;

use gtk4::prelude::*;
use gtk4::{self as gtk, glib};
use tokio::sync::mpsc;
use tracing::info;

use ui::window::{Command, Event, MainWindow};

const APP_ID: &str = "com.github.btswitch";

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Exclusive BT Switcher");

    let application = gtk::Application::builder()
        .application_id(APP_ID)
        .build();

    application.connect_activate(build_ui);

    // Run GTK application (handles single-instance via app ID)
    application.run();
}

fn build_ui(app: &gtk::Application) {
    // Command channel: UI → Backend (tokio mpsc, Send-safe)
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();

    // Event channel: Backend → UI (std mpsc, polled from GTK main loop)
    let (event_tx, event_rx) = std_mpsc::channel::<Event>();

    // Build the main window
    let cmd_tx_ui = cmd_tx.clone();
    let main_window = Rc::new(MainWindow::new(app, cmd_tx_ui));

    // Poll for backend events every 50ms
    let event_rx = RefCell::new(Some(event_rx));
    let win = main_window.clone();
    let app_quit = app.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let rx = event_rx.borrow();
        if let Some(rx) = rx.as_ref() {
            while let Ok(event) = rx.try_recv() {
                if matches!(event, Event::Quit) {
                    app_quit.quit();
                    return glib::ControlFlow::Break;
                }
                win.handle_event(event);
            }
        }
        glib::ControlFlow::Continue
    });

    // Spawn Tokio runtime in a background thread
    let event_tx_clone = event_tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        rt.block_on(async {
            // Set up signal handlers
            let cmd_tx_signal = cmd_tx.clone();
            tokio::spawn(async move {
                let mut sigint =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                        .expect("Failed to register SIGINT handler");
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("Failed to register SIGTERM handler");

                tokio::select! {
                    _ = sigint.recv() => {
                        info!("Received SIGINT");
                    }
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM");
                    }
                }

                let _ = cmd_tx_signal.send(Command::Shutdown);
            });

            // Run the backend
            app::run_backend(cmd_rx, event_tx_clone).await;
        });
    });

    // Set up panic hook for recovery marker
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Only write recovery marker — no D-Bus calls in panic context
        if let Ok(Some(st)) = state::AppState::load() {
            if !st.app_blocked_devices.is_empty() {
                let _ = state::AppState::save_in_progress(
                    st.exclusive_target.as_deref().unwrap_or(""),
                    &st.app_blocked_devices.iter().cloned().collect(),
                );
            }
        }
        default_hook(info);
    }));

    // System tray (best-effort, pure D-Bus via ksni — no GTK3)
    let cmd_tx_tray = main_window.cmd_sender().clone();
    ui::tray::spawn_tray(cmd_tx_tray);

    // When window is closed, minimize to tray instead of exiting
    main_window.window.connect_close_request(move |window| {
        window.minimize();
        glib::Propagation::Stop
    });

    main_window.window.present();
}
