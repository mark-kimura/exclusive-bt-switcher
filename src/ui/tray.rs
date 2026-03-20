// System tray support is disabled.
// The tray-icon crate uses libappindicator (GTK3), which cannot coexist
// with GTK4 in the same process (GLib type registration conflict).
// A future approach could use a pure D-Bus StatusNotifierItem implementation.
