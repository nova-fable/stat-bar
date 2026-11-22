#!/usr/bin/env bash
# 构建 StatBar.app：release 二进制 + Info.plist + AppIcon.icns，并做 ad-hoc 签名。
# 用法：scripts/bundle.sh（输出到 target/bundle/StatBar.app）
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME=StatBar
BIN_NAME=statbar
BUNDLE_ID=com.nova-fable.StatBar
ICON_SRC=${ICON_SRC:-assets/icon/AppIcon.png}
OUT_DIR=target/bundle
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

cargo build --release --locked

APP="$OUT_DIR/$APP_NAME.app"
CONTENTS="$APP/Contents"
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
cp "target/release/$BIN_NAME" "$CONTENTS/MacOS/$BIN_NAME"

ICONSET=$(mktemp -d)/AppIcon.iconset
mkdir "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/AppIcon.icns"

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key>
    <string>$BIN_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>27.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

codesign --force --sign - "$APP"
codesign --verify --verbose=2 "$APP"
echo "已生成 ${APP}（版本 ${VERSION}）"
