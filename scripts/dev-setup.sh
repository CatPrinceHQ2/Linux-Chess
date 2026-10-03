#!/bin/sh
# Installs build dependencies on Ubuntu / Pop!_OS 24.04 and the Rust toolchain via rustup.
set -e
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libgtk-4-dev libadwaita-1-dev git curl
if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  echo "Restart your shell (or: . \$HOME/.cargo/env) so cargo is on your PATH."
fi
echo "Optional: sudo apt-get install stockfish   # a default engine to try"
