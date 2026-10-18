#!/bin/zsh
set -euo pipefail
cd "$(dirname "$0")"
cargo build --locked --release
app_bundle="$PWD/Browser Recorder.app"
mkdir -p "$app_bundle/Contents/MacOS"
cp target/release/browser-recorder "$app_bundle/Contents/MacOS/BrowserRecorder"
cat > "$app_bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>BrowserRecorder</string>
<key>CFBundleIdentifier</key><string>local.muzik.browserrecorder</string>
<key>CFBundleName</key><string>Browser Recorder</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>2</string>
<key>CFBundleShortVersionString</key><string>1.1</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSScreenCaptureUsageDescription</key><string>Record audio from your selected browser into a local WAV file. No microphone or screen video is saved.</string>
</dict></plist>
PLIST
xattr -d com.apple.FinderInfo "$app_bundle" 2>/dev/null || true
codesign --force --sign - "$app_bundle"
echo "Built: $app_bundle"
