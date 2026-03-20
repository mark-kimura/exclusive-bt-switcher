# Exclusive BT Switcher

A Linux desktop app that lets you **exclusively lock one Bluetooth audio device** at a time. When you select a device, all other paired BT audio devices are blocked from reconnecting — no more headphones stealing audio from your speakers before a video call.

## The Problem

When multiple Bluetooth audio devices are paired on Linux, any of them can auto-connect and grab audio output at any time. This is especially annoying when:

- Your BT speaker reconnects and steals audio right before a meeting
- You switch headphones but the old pair keeps reconnecting
- You have to manually disconnect devices every time

## How It Works

1. Open the app — it lists all your paired Bluetooth audio devices
2. Click the device you want to use
3. That device becomes your active audio output (blue)
4. All other devices are blocked from reconnecting (red)
5. Audio streams are automatically migrated to the selected device

The block persists even after closing the app. Click "Unblock All" when you want to allow all devices to connect freely again.

## Screenshots

The app shows each device as a clickable row:
- **Blue** = Active (currently connected, receiving audio)
- **Red** = Blocked (cannot reconnect until you switch to it or unblock)
- **Default** = Disconnected (available to connect)

## Requirements

- **Linux** with BlueZ (tested on Linux Mint 22.3 / Cinnamon)
- **PipeWire** + **WirePlumber** (for audio routing)
- **GTK 4** runtime libraries
- Command-line tools: `pw-dump`, `wpctl`, `pactl` (usually installed with PipeWire)

## Building from Source

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install GTK 4 development headers and build tools
sudo apt install libgtk-4-dev build-essential pkg-config libxdo-dev
```

### Build and Run

```bash
git clone https://github.com/YOUR_USERNAME/exclusive-bt-switcher.git
cd exclusive-bt-switcher
cargo build --release
./target/release/exclusive-bt-switcher
```

### Install to PATH (optional)

```bash
cargo install --path .
```

## Technical Details

- **GTK 4** UI with Rust (gtk4-rs)
- **zbus 4** for D-Bus communication with BlueZ
- **Tokio** async runtime in a background thread for all BT/audio operations
- **Hybrid blocking strategy:**
  - Classic BT devices (BR/EDR): uses BlueZ `Blocked` property
  - BLE devices: uses `Trusted=false` + `Disconnect()` to avoid GATT cache corruption
- **Audio routing:** `pw-dump` for device discovery, `wpctl set-default` for sink switching, `pactl move-sink-input` for migrating active streams
- **Crash-safe:** state persisted to `~/.config/exclusive-bt-switcher/state.json` with atomic writes

## Troubleshooting

**App says "Cannot connect to BlueZ"**
- Make sure the Bluetooth service is running: `systemctl status bluetooth`

**Device shows "Disconnected" but won't connect**
- The device might be out of range or powered off
- Try turning the device off and on again

**Audio doesn't switch even though device shows "Active"**
- Check that PipeWire is running: `wpctl status`
- The PipeWire sink may take a few seconds to appear after BT connection

**BLE device takes longer to connect**
- This is normal. BLE devices need extra time after being unblocked. The app waits for auto-connect before attempting a manual connection.

## License

MIT
