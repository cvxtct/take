#!/bin/zsh
set -euo pipefail
cd "$(dirname "$0")"
cargo build --locked --release
app_bundle="$PWD/Take.app"
legacy_bundle="$PWD/Browser Recorder.app"
# Migrate the existing installation instead of leaving a second launchable app.
if [[ -d "$legacy_bundle" && ! -e "$app_bundle" ]]; then
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "$legacy_bundle" || true
  mv "$legacy_bundle" "$app_bundle"
fi
mkdir -p "$app_bundle/Contents/MacOS"
cp target/release/take "$app_bundle/Contents/MacOS/BrowserRecorder"
cat > "$app_bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>BrowserRecorder</string>
<key>CFBundleIdentifier</key><string>local.muzik.browserrecorder</string>
<key>CFBundleName</key><string>Take</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>3</string>
<key>CFBundleShortVersionString</key><string>1.1</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSScreenCaptureUsageDescription</key><string>Take needs screen-recording permission to list applications and capture audio from your selected app. No screen video or microphone audio is saved.</string>
</dict></plist>
PLIST
xattr -d com.apple.FinderInfo "$app_bundle" 2>/dev/null || true
codesign --force --sign - "$app_bundle"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app_bundle"
echo "Built: $app_bundle"
