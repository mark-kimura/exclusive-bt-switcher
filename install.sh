#!/bin/bash
set -e

ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
ICON_PATH="$ICON_DIR/btswitch.png"

echo "Building btswitch..."
cargo install --path . --force

echo "Installing icon..."
mkdir -p "$ICON_DIR"
cp resources/icons/btswitch256x256.png "$ICON_PATH"

echo "Installing desktop entry..."
sed "s|ICON_PATH_PLACEHOLDER|$ICON_PATH|" resources/btswitch.desktop \
    > ~/.local/share/applications/btswitch.desktop

echo "Done! 'Exclusive BT Switcher' is now in your app menu."
