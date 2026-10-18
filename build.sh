#!/bin/zsh
set -euo pipefail
cd "$(dirname "$0")"
# Ordinary builds are always public. Only --local reads the ignored setting.
build_mode=public
mode_option=""
print_mode=false
for option in "$@"; do
  case "$option" in
    --print-mode) print_mode=true ;;
    --public|--local)
      if [[ -n "$mode_option" ]]; then
        echo "Choose only one of --public or --local." >&2; exit 1
      fi
      mode_option="$option" ;;
    *) echo "Usage: zsh build.sh [--public|--local] [--print-mode]" >&2; exit 1 ;;
  esac
done
if [[ "$mode_option" == --local ]]; then
  if [[ ! -f .take-build-mode ]]; then
    echo "Create .take-build-mode containing public or extended before using --local." >&2; exit 1
  fi
  build_mode=$(<.take-build-mode)
  case "$build_mode" in
    public|extended) ;;
    *) echo "Invalid .take-build-mode: expected public or extended." >&2; exit 1 ;;
  esac
fi
if $print_mode; then
  echo "$build_mode"
  exit 0
fi
build_options=(--locked --release --no-default-features)
if [[ "$build_mode" == extended ]]; then
  build_options+=(--features extended-recording)
fi
echo "Building Take: $build_mode recording policy"
cargo build "${build_options[@]}"
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
echo "Built: $app_bundle ($build_mode recording policy)"
