#!/usr/bin/env bash
set -euo pipefail
bin=$1 tag=$2 out=$3
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
app="$work/dmg/Codescape.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 "$bin" "$app/Contents/MacOS/codescape"
sed "s/VERSION/${tag#v}/g" "$here/Info.plist" > "$app/Contents/Info.plist"
iconset="$work/codescape.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
  sips -z $size $size "$here/../icon/codescape.png" --out "$iconset/icon_${size}x${size}.png" > /dev/null
  sips -z $((size * 2)) $((size * 2)) "$here/../icon/codescape.png" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/codescape.icns"
ln -s /Applications "$work/dmg/Applications"
mkdir -p "$out"
hdiutil create -volname Codescape -srcfolder "$work/dmg" -ov -format UDZO "$out/Codescape-$tag-macos.dmg"
