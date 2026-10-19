#!/usr/bin/env bash
set -euo pipefail
bin=$1 tag=$2 tool=$3 out=$4
here=$(cd "$(dirname "$0")" && pwd)
app=$(mktemp -d)/Codescape.AppDir
mkdir -p "$app/usr/bin" "$app/usr/share/icons/hicolor/512x512/apps"
install -m755 "$bin" "$app/usr/bin/codescape"
ln -s usr/bin/codescape "$app/AppRun"
cp "$here/codescape.desktop" "$app/"
cp "$here/../icon/codescape.png" "$app/codescape.png"
cp "$here/../icon/codescape.png" "$app/usr/share/icons/hicolor/512x512/apps/codescape.png"
mkdir -p "$out"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$app" "$out/Codescape-$tag-x86_64.AppImage"
