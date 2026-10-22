#!/bin/sh
# Build and install the CLI and the app for the current user (~/.local).
set -eu
cd "$(dirname "$0")"
cargo build --release -p tankmini-cli -p tankmini-gui
install -Dm755 target/release/tankmini "$HOME/.local/bin/tankmini"
install -Dm755 target/release/tankmini-gui "$HOME/.local/bin/tankmini-gui"
install -Dm644 data/dev.v1ni.TankMini.desktop "$HOME/.local/share/applications/dev.v1ni.TankMini.desktop"
echo "instalado: tankmini, tankmini-gui e o atalho no menu"
