#!/bin/sh
# Build dist/QuietAutoClicker.app. Set SIGN_IDENTITY to sign with a real
# certificate; the default is an ad-hoc signature.
set -eu
cd "$(dirname "$0")/.."

name=QuietAutoClicker
app="dist/$name.app"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
target="${CARGO_TARGET_DIR:-target}"

cargo build --release

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target/release/$name" "$app/Contents/MacOS/"

icons=$(mktemp -d)/icon.iconset
mkdir "$icons"
for s in 16 32 128 256; do
    sips -z $s $s ui/icons/QAC_Logo_256.png --out "$icons/icon_${s}x${s}.png" >/dev/null
done
iconutil -c icns "$icons" -o "$app/Contents/Resources/$name.icns"
rm -rf "$(dirname "$icons")"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>$name</string>
    <key>CFBundleIconFile</key><string>$name</string>
    <key>CFBundleIdentifier</key><string>io.github.bequietbro.$name</string>
    <key>CFBundleName</key><string>$name</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSAppNapIsDisabled</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

# Hardened runtime keeps other processes from injecting code into an app that
# holds Accessibility access.
codesign --force --options runtime --sign "${SIGN_IDENTITY:--}" "$app"
echo "Built $app"
