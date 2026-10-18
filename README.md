# Take

Capture audio.

A small macOS desktop recorder for browser audio excerpts. Native AppKit controls written in Rust (system fonts, menus and keyboard shortcuts), direct Objective-C bindings to Apple's ScreenCaptureKit, and local WAV encoding (hound). No Swift bridge, browser extension, virtual audio driver, analytics, or application network requests. Cargo downloads open-source dependencies at build time; versions are pinned in Cargo.lock.

## Status and installation

Early-stage macOS application, currently distributed as source. Build locally
with Rust and Apple's command line tools before opening the app:

```sh
git clone https://github.com/cvxtct/take.git
cd take
zsh build.sh
open Take.app
```

Local builds are ad-hoc signed, not Developer ID signed or notarized. macOS may
ask you to authorize screen/system-audio capture again after a rebuild. Quit
and reopen the app after changing its permission. Keep one installed app copy
to avoid ambiguous Spotlight results and permission entries. No need to disable
macOS security protections.

Browser/app audio capture is implemented. KO II USB audio input, clip renaming,
and automatic key estimation are planned features, not currently available.

## Run

Open **Take.app** in this folder.

1. Open the browser and music you want to record.
2. Click **Refresh apps**. Allow capture when macOS requests it. If needed, enable Take in System Settings → Privacy & Security → Screen & System Audio Recording, then quit and reopen the app.
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
open "Take.app"
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

The macOS bundle identifier remains `local.muzik.browserrecorder` for continuity with existing installations. The build script migrates `Browser Recorder.app` to `Take.app`; recordings stay in the same folder.

## License

MIT — see [LICENSE](LICENSE). Third-party dependencies retain their own licenses.
## Security checks

The Security workflow runs on pull requests, main pushes, and weekly to catch
new advisories even without code changes. Semgrep Community Edition scans Rust
source with the community `p/rust` rules; cargo-deny checks Cargo.lock against
RustSec. Findings fail their job. Results are available in Actions logs.
Scans run on Linux with read-only repository permissions and no paid service
account. Semgrep fetches community rules, but scans on the runner with
metrics disabled; source is not uploaded to the Semgrep service. The rules and
advisory database are refreshed so new detections can appear over time.

Dependabot proposes weekly Cargo and GitHub Actions updates. Semgrep's pinned
CLI version should be reviewed periodically. Existing Clippy and tests remain
on macOS. These checks catch known patterns and dependency advisories, not all
vulnerabilities: unsafe Rust, Objective-C FFI, permissions, and audio-buffer
lifetimes still need manual review.

If Take appears only under **System Audio Recording Only**, click **Refresh**
in Take to explicitly request the screen-capture permission needed to enumerate
apps. Audio-only authorization does not satisfy this listing operation. Approve
the system prompt, then quit and reopen Take. The app does not reset permissions
or change macOS security settings itself. Local ad-hoc signing still means a
rebuild can require renewed authorization.
