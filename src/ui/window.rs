use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{self as gtk, Align, ApplicationWindow, Label, Orientation, PolicyType, ScrolledWindow};
use tokio::sync::mpsc;

use crate::bluetooth::device::BtAudioDevice;
use crate::ui::device_row;

/// Messages the UI sends to the backend
#[derive(Debug, Clone)]
pub enum Command {
    SwitchTo(String),
    ReleaseAll,
    Refresh,
    Shutdown,
}

/// Messages the backend sends to the UI
#[derive(Debug, Clone)]
pub enum Event {
    DeviceListUpdated(Vec<BtAudioDevice>),
    SwitchStarted,
    SwitchComplete,
    Error(String),
    AdapterPowered(bool),
    Quit,
}

pub struct MainWindow {
    pub window: ApplicationWindow,
    device_list_box: gtk::ListBox,
    spinner: gtk::Spinner,
    status_label: Label,
    bt_off_banner: gtk::Box,
    release_btn: gtk::Button,
    refresh_btn: gtk::Button,
    is_busy: Rc<RefCell<bool>>,
    cmd_sender: mpsc::UnboundedSender<Command>,
}

impl MainWindow {
    pub fn new(app: &gtk::Application, cmd_sender: mpsc::UnboundedSender<Command>) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Exclusive BT Switcher")
            .default_width(420)
            .default_height(500)
            .build();

        // Header bar
        let header = gtk::HeaderBar::new();

        let refresh_btn = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh_btn.set_tooltip_text(Some("Refresh device list"));
        header.pack_start(&refresh_btn);

        window.set_titlebar(Some(&header));

        // Main content
        let content = gtk::Box::new(Orientation::Vertical, 0);

        // BT off banner (hidden by default)
        let bt_off_banner = gtk::Box::new(Orientation::Horizontal, 8);
        bt_off_banner.set_margin_start(12);
        bt_off_banner.set_margin_end(12);
        bt_off_banner.set_margin_top(8);
        bt_off_banner.set_margin_bottom(8);
        bt_off_banner.add_css_class("warning");
        bt_off_banner.set_visible(false);

        let bt_off_icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
        bt_off_banner.append(&bt_off_icon);
        let bt_off_label = Label::new(Some("Bluetooth is turned off"));
        bt_off_label.set_hexpand(true);
        bt_off_label.set_halign(Align::Start);
        bt_off_banner.append(&bt_off_label);
        content.append(&bt_off_banner);

        // Spinner + status
        let status_box = gtk::Box::new(Orientation::Horizontal, 8);
        status_box.set_margin_start(12);
        status_box.set_margin_end(12);
        status_box.set_margin_top(4);
        status_box.set_margin_bottom(4);

        let spinner = gtk::Spinner::new();
        spinner.set_visible(false);
        status_box.append(&spinner);

        let status_label = Label::new(None);
        status_label.set_halign(Align::Start);
        status_label.add_css_class("dim-label");
        status_box.append(&status_label);
        content.append(&status_box);

        // Scrolled device list
        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);

        let device_list_box = gtk::ListBox::new();
        device_list_box.set_selection_mode(gtk::SelectionMode::None);
        device_list_box.add_css_class("boxed-list");
        scrolled.set_child(Some(&device_list_box));
        content.append(&scrolled);

        // Release All button at the bottom
        let release_btn = gtk::Button::with_label("Release All");
        release_btn.set_tooltip_text(Some("Unblock all devices blocked by this app"));
        release_btn.add_css_class("destructive-action");
        release_btn.set_margin_start(12);
        release_btn.set_margin_end(12);
        release_btn.set_margin_top(8);
        release_btn.set_margin_bottom(12);
        content.append(&release_btn);

        window.set_child(Some(&content));

        // Connect button signals
        let cmd = cmd_sender.clone();
        refresh_btn.connect_clicked(move |_| {
            let _ = cmd.send(Command::Refresh);
        });

        let cmd = cmd_sender.clone();
        release_btn.connect_clicked(move |_| {
            let _ = cmd.send(Command::ReleaseAll);
        });

        Self {
            window,
            device_list_box,
            spinner,
            status_label,
            bt_off_banner,
            release_btn,
            refresh_btn,
            is_busy: Rc::new(RefCell::new(false)),
            cmd_sender,
        }
    }

    pub fn cmd_sender(&self) -> &mpsc::UnboundedSender<Command> {
        &self.cmd_sender
    }

    /// Handle an event from the backend
    pub fn handle_event(&self, event: Event) {
        match event {
            Event::DeviceListUpdated(devices) => {
                self.update_device_list(&devices);
            }
            Event::SwitchStarted => {
                *self.is_busy.borrow_mut() = true;
                self.spinner.set_visible(true);
                self.spinner.start();
                self.status_label.set_text("Switching...");
                self.release_btn.set_sensitive(false);
                self.refresh_btn.set_sensitive(false);
            }
            Event::SwitchComplete => {
                *self.is_busy.borrow_mut() = false;
                self.spinner.stop();
                self.spinner.set_visible(false);
                self.status_label.set_text("");
                self.release_btn.set_sensitive(true);
                self.refresh_btn.set_sensitive(true);
            }
            Event::Error(msg) => {
                *self.is_busy.borrow_mut() = false;
                self.spinner.stop();
                self.spinner.set_visible(false);
                self.status_label.set_text(&format!("Error: {msg}"));
                self.release_btn.set_sensitive(true);
                self.refresh_btn.set_sensitive(true);

                // Show error dialog
                let dialog = gtk::MessageDialog::builder()
                    .message_type(gtk::MessageType::Error)
                    .buttons(gtk::ButtonsType::Ok)
                    .text("Error")
                    .secondary_text(&msg)
                    .transient_for(&self.window)
                    .modal(true)
                    .build();
                dialog.connect_response(|dlg, _| dlg.close());
                dialog.present();
            }
            Event::AdapterPowered(powered) => {
                self.bt_off_banner.set_visible(!powered);
            }
            Event::Quit => {
                // Handled in main.rs event polling loop
            }
        }
    }

    fn update_device_list(&self, devices: &[BtAudioDevice]) {
        // Remove all existing rows
        while let Some(child) = self.device_list_box.first_child() {
            self.device_list_box.remove(&child);
        }

        if devices.is_empty() {
            let label = Label::new(Some("No paired Bluetooth audio devices found"));
            label.set_margin_top(24);
            label.set_margin_bottom(24);
            label.add_css_class("dim-label");
            self.device_list_box.append(&label);
            return;
        }

        let is_busy = *self.is_busy.borrow();
        // A device is "exclusive" if it's the only connected one (others are blocked/disconnected)
        let connected_count = devices.iter().filter(|d| d.status == crate::bluetooth::device::DeviceStatus::Connected).count();
        for device in devices {
            let is_exclusive = device.status == crate::bluetooth::device::DeviceStatus::Connected && connected_count == 1;
            let cmd = self.cmd_sender.clone();
            let row = device_row::build_device_row(device, is_busy, is_exclusive, move |path| {
                let _ = cmd.send(Command::SwitchTo(path));
            });
            self.device_list_box.append(&row);
        }
    }
}
