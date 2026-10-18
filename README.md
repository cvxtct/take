# Browser Recorder (Rust)

A small macOS desktop recorder for browser audio excerpts. Native AppKit controls written in Rust (system fonts, menus and keyboard shortcuts), direct Objective-C bindings to Apple's ScreenCaptureKit, and local WAV encoding (hound). No Swift bridge, browser extension, virtual audio driver, analytics, or application network requests. Cargo downloads open-source dependencies at build time; versions are pinned in Cargo.lock.

## Run

Open **Browser Recorder.app** in this folder.

1. Open the browser and music you want to record.
2. Click **Refresh apps**. Allow capture when macOS requests it. If needed, enable Browser Recorder in System Settings → Privacy & Security → Screen & System Audio Recording, then quit and reopen the app.
3. Select your browser. Capture is application-wide: pause other tabs in that browser.
4. Set a duration as minutes:seconds (e.g. `7:55`) or whole seconds (e.g. `475`), from `0:01` to `60:00`, or enable **Until I stop** to record without a timer. Choose a save folder. Default: `recordings` beside the app.
5. Click **Record**, then play the excerpt. An elapsed timer and live level history show progress.
6. Timed mode stops automatically. In manual mode, click **Stop & save** when finished; the display shows elapsed time. Use **Play** or **Show in Finder** to review the WAV.

Manual mode has no time limit, but a single WAV is saved automatically at the RIFF file size limit (about 6 hours at this format) to avoid corrupting the file.

Output: lossless, stereo, 48 kHz, 16-bit WAV (about 5.8 MB for 30 seconds). Files have unique timestamp names; existing files are never overwritten. Silent captures are reported. Failed captures are removed instead of being presented as complete. Closing the window during recording requests stop and save first.

Only the audio output is registered with ScreenCaptureKit. The capture framework still requires macOS screen/system-audio permission; the app never receives video frames and never enables microphone capture. Protected playback may be unavailable. The timer is wall-clock based; startup and packet boundaries can slightly affect actual audio length. This is an excerpt recorder, not a sample-exact musical loop editor.

## Build and check

Requires macOS 13+, Rust and Apple's command line tools.

```sh
cargo test --locked
cargo clippy --locked -- -D warnings
zsh build.sh
open "Browser Recorder.app"
```

`src/main.rs`: GUI and worker lifecycle.
`src/capture.rs`: app enumeration, capture, PCM conversion, WAV output and tests.

Live acceptance check (requires user-approved macOS capture permission): record 10 seconds from a browser playing audio; verify the waveform meter, resulting file's audio and approximate duration. Check Stop & save with a longer duration and try a silent source. The unit tests check duration validation, stereo channel ordering and PCM conversion.

## Recorder interface

Dark two-panel AppKit interface with an elapsed timer, live peak-level history, duration presets, and a scrollable WAV recordings library. Select a clip, then use Play or Show in Finder. Reload scans the save folder again. The live display shows recent signal levels, not a sample-accurate waveform.

## Contributing

Start each change with a GitHub issue and a dedicated work branch from `main`.
Submit a pull request linked to the issue. CI runs formatting, unit tests, and
Clippy on macOS for pull requests and pushes to `main`. See `AGENTS.md`.

## Credits

- **[cvxtct](https://github.com/cvxtct)** — project concept, specification, creative direction, and hands-on testing.
- **OpenAI Codex (AI coding assistant)** — implementation, native macOS UI development, automated tests, and development tooling, guided and reviewed by cvxtct.
