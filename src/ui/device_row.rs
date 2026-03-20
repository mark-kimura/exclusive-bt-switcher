use gtk4::prelude::*;
use gtk4::{self as gtk, Align, Label, Orientation};

use crate::bluetooth::device::{BtAudioDevice, DeviceStatus};

/// Create a GTK button row for a Bluetooth audio device.
/// The entire row is a clickable button. Background color indicates status.
pub fn build_device_row(
    device: &BtAudioDevice,
    is_busy: bool,
    is_exclusive: bool,
    on_switch: impl Fn(String) + 'static,
) -> gtk::Button {
    // Inner layout: icon + name on the left, status on the right
    let row = gtk::Box::new(Orientation::Horizontal, 12);
    row.set_margin_start(8);
    row.set_margin_end(8);
    row.set_margin_top(6);
    row.set_margin_bottom(6);

    // Icon
    let icon_name = match device.icon.as_deref() {
        Some("audio-headphones") => "audio-headphones-symbolic",
        Some("audio-headset") => "audio-headset-symbolic",
        _ => "audio-speakers-symbolic",
    };
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(24);
    row.append(&icon);

    // Device name
    let name_label = Label::new(Some(&device.alias));
    name_label.set_halign(Align::Start);
    name_label.set_hexpand(true);
    row.append(&name_label);

    // Status text on the right
    let status_text = match device.status {
        DeviceStatus::Connected if is_exclusive => "Active",
        DeviceStatus::Connected => "Connected",
        DeviceStatus::Blocked => "Blocked",
        DeviceStatus::Disconnected => "Disconnected",
        DeviceStatus::Connecting => "Connecting…",
    };
    let status_label = Label::new(Some(status_text));
    row.append(&status_label);

    // The whole row is a button
    let button = gtk::Button::new();
    button.set_child(Some(&row));
    button.add_css_class("device-row");

    // Style based on status
    if is_exclusive {
        button.add_css_class("device-active");
        button.set_sensitive(false);
    } else {
        match device.status {
            DeviceStatus::Blocked => {
                button.add_css_class("device-blocked");
            }
            _ => {
                button.add_css_class("flat");
            }
        }
    }

    if is_busy {
        button.set_sensitive(false);
    }

    let path = device.path.clone();
    button.connect_clicked(move |_| {
        on_switch(path.clone());
    });

    button
}
