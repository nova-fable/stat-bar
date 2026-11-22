#!/usr/bin/env bash
# 生成拖拽安装的 DMG：StatBar.app + Applications 快捷方式 + 背景图。
# 用法：scripts/make-dmg.sh <输出.dmg>（需先运行 scripts/bundle.sh）
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=${1:?用法：scripts/make-dmg.sh <输出.dmg>}
APP=target/bundle/StatBar.app
BG=${BG_SRC:-assets/dmg/background.png}
BG2X=${BG2X_SRC:-assets/dmg/background@2x.png}
VOL=StatBar

[ -d "${APP}" ] || { echo "缺少 ${APP}，请先运行 scripts/bundle.sh" >&2; exit 1; }
[ -e "/Volumes/$VOL" ] && { echo "/Volumes/$VOL 已挂载，请先弹出后再运行" >&2; exit 1; }

WORK=$(mktemp -d)
STAGE="$WORK/stage"
RW="$WORK/rw.dmg"
mkdir -p "$STAGE/.background"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

BG_NAME=""
if [ -f "$BG" ] && [ -f "$BG2X" ]; then
  tiffutil -cathidpicheck "$BG" "$BG2X" -out "$STAGE/.background/background.tiff"
  BG_NAME=background.tiff
elif [ -f "$BG" ]; then
  cp "$BG" "$STAGE/.background/background.png"
  BG_NAME=background.png
fi

hdiutil create -quiet -srcfolder "$STAGE" -volname "$VOL" -fs HFS+ -format UDRW -ov "$RW"
hdiutil attach -quiet -readwrite -noverify -noautoopen "$RW"

BG_LINE=""
if [ -n "$BG_NAME" ]; then
  BG_LINE="set background picture of viewOptions to file \".background:${BG_NAME}\""
fi

if ! osascript <<EOF
tell application "Finder"
  tell disk "$VOL"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set bounds of container window to {100, 100, 760, 500}
    set viewOptions to the icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 128
    $BG_LINE
    set position of item "StatBar.app" of container window to {160, 200}
    set position of item "Applications" of container window to {500, 200}
    update without registering applications
    delay 1
    close
  end tell
end tell
EOF
then
  echo "警告：未能通过 Finder 设置窗口布局，DMG 将使用默认布局" >&2
fi

sync
hdiutil detach -quiet "/Volumes/$VOL" || hdiutil detach -quiet -force "/Volumes/$VOL"

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT"
hdiutil convert -quiet "$RW" -format UDZO -imagekey zlib-level=9 -o "$OUT"
rm -rf "$WORK"
echo "已生成 $OUT"
