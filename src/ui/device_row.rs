use gtk4::prelude::*;
use gtk4::{self as gtk, Align, Label, Orientation};

use crate::bluetooth::device::{BtAudioDevice, DeviceStatus};

/// Create a GTK widget row for a Bluetooth audio device
pub fn build_device_row(
    device: &BtAudioDevice,
    is_busy: bool,
    is_exclusive: bool,
    on_switch: impl Fn(String) + 'static,
) -> gtk::Box {
    let row = gtk::Box::new(Orientation::Horizontal, 12);
    row.set_margin_start(12);
    row.set_margin_end(12);
    row.set_margin_top(8);
    row.set_margin_bottom(8);

    // Icon
    let icon_name = match device.icon.as_deref() {
        Some("audio-headphones") => "audio-headphones-symbolic",
        Some("audio-headset") => "audio-headset-symbolic",
        _ => "audio-speakers-symbolic",
    };
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(24);
    row.append(&icon);

    // Device name + address
    let info_box = gtk::Box::new(Orientation::Vertical, 2);
    info_box.set_hexpand(true);
    info_box.set_valign(Align::Center);

    let name_label = Label::new(Some(&device.alias));
    name_label.set_halign(Align::Start);
    name_label.add_css_class("heading");
    info_box.append(&name_label);

    let addr_label = Label::new(Some(&device.address));
    addr_label.set_halign(Align::Start);
    addr_label.add_css_class("dim-label");
    addr_label.add_css_class("caption");
    info_box.append(&addr_label);

    row.append(&info_box);

    // Status badge
    let badge = Label::new(Some(&device.status.to_string()));
    badge.set_valign(Align::Center);
    match device.status {
        DeviceStatus::Connected => {
            badge.add_css_class("success");
        }
        DeviceStatus::Connecting => {
            badge.add_css_class("warning");
        }
        DeviceStatus::Blocked => {
            badge.add_css_class("error");
        }
        DeviceStatus::Disconnected => {
            badge.add_css_class("dim-label");
        }
    }
    row.append(&badge);

    // Switch button — disabled only if this device is the exclusive active one
    let button = gtk::Button::with_label(if is_exclusive { "  Active  " } else { "  Switch  " });
    button.set_valign(Align::Center);
    button.set_margin_start(8);

    if is_exclusive {
        button.add_css_class("suggested-action");
        button.set_sensitive(false);
    }

    if is_busy {
        button.set_sensitive(false);
    }

    let path = device.path.clone();
    let alias = device.alias.clone();
    button.connect_clicked(move |_| {
        eprintln!("[UI] Switch clicked for {alias} at {path}");
        on_switch(path.clone());
    });

    row.append(&button);

    row
}
