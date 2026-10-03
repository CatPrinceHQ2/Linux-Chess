#!/bin/sh
# Builds dist/linux-chess_<version>_amd64.deb from a release build.
# Needs: Rust >= 1.85, libgtk-4-dev, libadwaita-1-dev, pkg-config, python3-pil (icon resizing).
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
PKG=linux-chess
ID=io.github.catprincehq.linux_chess
ROOT=dist/deb-root
rm -rf "$ROOT"
cargo build --release --locked
install -Dm755 target/release/$PKG "$ROOT/usr/bin/$PKG"
install -Dm644 data/$ID.desktop "$ROOT/usr/share/applications/$ID.desktop"
for s in 48 64 128 256 512; do
  mkdir -p "$ROOT/usr/share/icons/hicolor/${s}x${s}/apps"
  python3 -c "
from PIL import Image
Image.open('data/$ID.png').convert('RGBA').resize(($s,$s),Image.LANCZOS).save('$ROOT/usr/share/icons/hicolor/${s}x${s}/apps/$ID.png',optimize=True)"
done
install -Dm644 LICENSE "$ROOT/usr/share/doc/$PKG/copyright"
install -Dm644 THIRD_PARTY_ENGINES.md "$ROOT/usr/share/doc/$PKG/THIRD_PARTY_ENGINES.md"
mkdir -p "$ROOT/DEBIAN"
SIZE=$(du -sk "$ROOT" | cut -f1)
cat > "$ROOT/DEBIAN/control" <<CTL
Package: $PKG
Version: $VERSION
Section: games
Priority: optional
Architecture: amd64
Maintainer: CatPrinceHQ <noreply@catprincehq.invalid>
Installed-Size: $SIZE
Depends: libgtk-4-1 (>= 4.10), libadwaita-1-0 (>= 1.4), libc6
Recommends: stockfish
Description: Prince's Linux-Chess - play chess against a bot locally
 Allows you to play chess against a bot locally on your Linux machine with
 performance in mind. Works for Ubuntu/Debian systems - roughly emulates the
 experience of chess-bot.com but works offline and locally with a file size of
 less than one megabyte.
CTL
mkdir -p dist
dpkg-deb -Zxz --build --root-owner-group "$ROOT" "dist/${PKG}_${VERSION}_amd64.deb"
echo "Built dist/${PKG}_${VERSION}_amd64.deb"
