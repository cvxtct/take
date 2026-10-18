use crate::recording_policy::{self, CaptureLimits, WAV_MAX_FRAMES};
use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::{
    AnyThread, DefinedClass, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_core_audio_types::*;
use objc2_core_media::{CMAudioFormatDescriptionGetStreamBasicDescription, CMSampleBuffer};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::*;
use std::{
    fs::{File, OpenOptions},
    io::BufWriter,
    path::PathBuf,
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct AppInfo {
    pub pid: i32,
    pub name: String,
    pub browser: bool,
    pub bundle: String,
}
pub enum Event {
    Apps(Vec<AppInfo>),
    Started,
    Level(f32),
    Saved(PathBuf, String),
    Error(String),
}
pub fn parse_duration(text: &str) -> Result<u64, String> {
    let input = text.trim();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let duration = if let Some((minutes, seconds)) = input.split_once(':') {
        if digits(minutes) && seconds.len() == 2 && digits(seconds) {
            minutes.parse::<u64>().ok().and_then(|m| {
                seconds
                    .parse::<u64>()
                    .ok()
                    .filter(|s| *s < 60)
                    .and_then(|s| m.checked_mul(60)?.checked_add(s))
            })
        } else {
            None
        }
    } else if digits(input) {
        input.parse::<u64>().ok()
    } else {
        None
    };
    duration.filter(|n| *n > 0).ok_or_else(||
        "Enter a positive duration in minutes:seconds (e.g. 7:55) or whole seconds. Use two seconds digits, 00–59; the value must fit a 64-bit unsigned integer.".into())
}
pub fn recording_duration(
    bundle: &str,
    text: &str,
    until_stopped: bool,
) -> Result<Option<u64>, String> {
    let duration = if until_stopped {
        None
    } else {
        Some(parse_duration(text)?)
    };
    recording_policy::validate_duration(bundle, duration)?;
    Ok(duration)
}
unsafe fn ns_error(error: *mut NSError) -> Option<String> {
    // SAFETY: Apple supplies either a valid NSError for this callback or null.
    // SAFETY (#16): Caller guarantees a live callback NSError or null; its description is copied before callback return.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe { error.as_ref().map(|e| e.localizedDescription().to_string()) }
}
fn content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |value: *mut SCShareableContent, error: *mut NSError| {
        // SAFETY (#16): ScreenCaptureKit supplies a valid NSError or null for the duration of this completion callback.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let result = if let Some(e) = unsafe { ns_error(error) } {
            Err(e)
        } else {
            // Retain the immutable content snapshot before Apple's callback returns.
            // SAFETY (#16): Apple callback object is live here; retain extends its lifetime beyond callback return and null becomes an error.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            unsafe { Retained::retain(value) }.ok_or_else(|| "No capture sources returned.".into())
        };
        let _ = tx.send(result);
    });
    // SAFETY (#16): ScreenCaptureKit copies the completion block; captured sender owns its state, so late completion after timeout is safe.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, false, &block);
    }
    rx.recv_timeout(Duration::from_secs(60)).map_err(|_| {
        "Capture permission timed out. Allow Take in System Settings, then try again.".to_string()
    })?
}
pub fn list_apps(tx: mpsc::Sender<Event>) {
    // SAFETY (#16): Retained shareable snapshot owns the applications while immutable properties are copied into Rust values.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let result = content().map(|content| unsafe {
        let mut apps: Vec<_> = content
            .applications()
            .iter()
            .filter_map(|app| {
                let name = app.applicationName().to_string();
                let pid = app.processID();
                if name.is_empty() || pid == std::process::id() as i32 {
                    return None;
                }
                let bundle = app.bundleIdentifier().to_string();
                Some(AppInfo {
                    pid,
                    name,
                    browser: recording_policy::is_browser(&bundle),
                    bundle,
                })
            })
            .collect();
        apps.sort_by_key(|a| (!a.browser, a.name.to_lowercase()));
        apps
    });
    let _ = tx.send(match result {
        Ok(apps) => Event::Apps(apps),
        Err(e) => Event::Error(format!("Cannot list apps: {e}\nAllow capture in System Settings → Privacy & Security → Screen & System Audio Recording, then reopen this app.")),
    });
}
type Writer = hound::WavWriter<BufWriter<File>>;
struct AudioState {
    writer: Option<Writer>,
    frames: u64,
    limits: CaptureLimits,
    peak: f32,
    error: Option<String>,
    last_meter: Instant,
    tx: mpsc::Sender<Event>,
}
#[repr(C)]
struct StereoBufferList {
    count: u32,
    buffers: [AudioBuffer; 2],
}

fn supported_pcm_format(asbd: &AudioStreamBasicDescription) -> bool {
    !(asbd.mFormatID != kAudioFormatLinearPCM
        || asbd.mFormatFlags & kAudioFormatFlagIsFloat == 0
        || asbd.mFormatFlags & kAudioFormatFlagIsBigEndian != 0
        || asbd.mFormatFlags & kAudioFormatFlagIsPacked == 0
        || asbd.mBytesPerFrame
            != if asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0 {
                4
            } else {
                8
            }
        || asbd.mBitsPerChannel != 32
        || asbd.mChannelsPerFrame != 2
        || asbd.mSampleRate != 48000.)
}

// Copy Apple's PCM into owned memory; accept only the explicitly requested format.
fn pcm(sample: &CMSampleBuffer) -> Result<Vec<f32>, String> {
    // SAFETY (#16): Sample is borrowed only during its callback. Format and frame bounds are checked before copying into aligned, owned float buffers; no borrowed audio escapes.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        let desc = sample.format_description().ok_or("Missing audio format")?;
        let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(&desc)
            .as_ref()
            .ok_or("Missing PCM description")?;
        if !supported_pcm_format(asbd) {
            return Err("Unexpected capture format; expected 48 kHz stereo float PCM.".into());
        }
        let frames = sample.num_samples();
        if !(1..=480_000).contains(&frames) {
            return Err("Invalid audio frame count".into());
        }
        let frames = frames as usize;
        let planar = asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
        let mut first = vec![0f32; if planar { frames } else { frames * 2 }];
        let mut second = vec![0f32; if planar { frames } else { 0 }];
        let mut list = StereoBufferList {
            count: if planar { 2 } else { 1 },
            buffers: [
                AudioBuffer {
                    mNumberChannels: if planar { 1 } else { 2 },
                    mDataByteSize: (first.len() * 4) as u32,
                    mData: first.as_mut_ptr().cast(),
                },
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: (second.len() * 4) as u32,
                    mData: second.as_mut_ptr().cast(),
                },
            ],
        };
        // repr(C) matches the variable-length AudioBufferList ABI. Both buffers remain alive and sized for the complete packet during this copy.
        let status = sample.copy_pcm_data_into_audio_buffer_list(
            0,
            frames as i32,
            NonNull::from(&mut list).cast(),
        );
        if status != 0 {
            return Err(format!("Audio copy failed ({status})"));
        }
        if planar {
            Ok(interleave(&first, &second))
        } else {
            Ok(first)
        }
    }
}
fn interleave(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter().zip(right).flat_map(|(&l, &r)| [l, r]).collect()
}
fn quantize(value: f32) -> i16 {
    if !value.is_finite() {
        0
    } else {
        (value.clamp(-1., 1.) * 32767.).round() as i16
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = Arc<Mutex<AudioState>>]
    struct AudioOutput;
    unsafe impl NSObjectProtocol for AudioOutput {}
    unsafe impl SCStreamOutput for AudioOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn output(&self, _stream: &SCStream, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
            if kind != SCStreamOutputType::Audio { return; }
            let Ok(mut state) = self.ivars().lock() else { return; };
            if state.writer.is_none() || state.error.is_some() || state.frames >= state.limits.max_frames { return; }
            match pcm(sample) {
                Err(e) => state.error = Some(e),
                Ok(samples) => {
                    let count = state.limits.samples_to_write(state.frames, samples.len());
                    let mut peak = 0f32;
                    for &sample in &samples[..count] {
                        peak = peak.max(sample.abs());
                        if let Err(e) = state.writer.as_mut().unwrap().write_sample(quantize(sample)) {
                            state.error = Some(e.to_string()); return;
                        }
                    }
                    state.frames += (count/2) as u64;
                    state.peak = state.peak.max(peak);
                    if state.last_meter.elapsed() > Duration::from_millis(100) {
                        let _ = state.tx.send(Event::Level(peak)); state.last_meter = Instant::now();
                    }
                }
            }
        }
    }
    unsafe impl SCStreamDelegate for AudioOutput {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stopped(&self, _stream: &SCStream, error: &NSError) {
            if let Ok(mut state) = self.ivars().lock() { state.error = Some(error.localizedDescription().to_string()); }
        }
    }
);
fn wait_capture(stream: &SCStream, start: bool) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |error: *mut NSError| {
        // SAFETY (#16): Completion-handler NSError is valid until return; only its owned description crosses threads.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let _ = tx.send(unsafe { ns_error(error) });
    });
    // SAFETY (#16): Stream is retained by the caller; Apple copies the block whose sender remains valid even after the receiver times out.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        if start {
            stream.startCaptureWithCompletionHandler(Some(&block));
        } else {
            stream.stopCaptureWithCompletionHandler(Some(&block));
        }
    }
    match rx.recv_timeout(Duration::from_secs(15)) {
        Ok(None) => Ok(()),
        Ok(Some(e)) => Err(e),
        Err(_) => Err("Capture operation timed out.".into()),
    }
}
pub fn record(
    pid: i32,
    duration: Option<u64>,
    folder: PathBuf,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
) {
    match record_inner(pid, duration, folder, stop, tx.clone()) {
        Ok((path, warning)) => {
            let _ = tx.send(Event::Saved(path, warning));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(e));
        }
    }
}
fn record_inner(
    pid: i32,
    duration: Option<u64>,
    folder: PathBuf,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
) -> Result<(PathBuf, String), String> {
    let content = content()?;
    // SAFETY (#16): Retained content snapshot is live; returned applications array is retained.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let apps = unsafe { content.applications() };
    let app = apps
        .iter()
        // SAFETY (#16): Each application is borrowed from the live retained array during iteration.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        .find(|a| unsafe { a.processID() } == pid)
        .ok_or("Selected app closed; refresh the list.")?;
    // SAFETY (#31): app is retained by the live shareable-content snapshot;
    // copy its immutable bundle identifier before starting capture.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let bundle = unsafe { app.bundleIdentifier().to_string() };
    let limits = CaptureLimits::new(&bundle, duration)?;
    // SAFETY (#16): Retained content snapshot is live; returned displays array is retained.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let displays = unsafe { content.displays() };
    let display = displays.firstObject().ok_or("No display available")?;
    if stop.load(Ordering::Relaxed) {
        return Err("Cancelled.".into());
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("Cannot create save folder: {e}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = folder.join(format!("Rhythm-{stamp}.wav"));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let writer = hound::WavWriter::new(
        BufWriter::new(file),
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|e| e.to_string())?;
    let state = Arc::new(Mutex::new(AudioState {
        writer: Some(writer),
        frames: 0,
        limits,
        peak: 0.,
        error: None,
        last_meter: Instant::now(),
        tx: tx.clone(),
    }));
    // SAFETY (#16): Retained filter/config/output/queue outlive stream use; callbacks own Arc state guarded by a mutex. No video output is registered.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let result = (|| unsafe {
        let output: Retained<AudioOutput> =
            msg_send![super(AudioOutput::alloc().set_ivars(state.clone())), init];
        let filter = SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::from_slice(&[&*app]),
            &NSArray::new(),
        );
        let config = SCStreamConfiguration::new();
        config.setCapturesAudio(true);
        config.setSampleRate(48000);
        config.setChannelCount(2);
        config.setExcludesCurrentProcessAudio(true);
        config.setWidth(2);
        config.setHeight(2);
        let stream = SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            Some(ProtocolObject::from_ref(&*output)),
        );
        let queue = DispatchQueue::new("local.browser-recorder.audio", None);
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*output),
                SCStreamOutputType::Audio,
                Some(&queue),
            )
            .map_err(|e| e.localizedDescription().to_string())?;
        if let Err(error) = wait_capture(&stream, true) {
            let _ = wait_capture(&stream, false);
            return Err(error);
        }
        let _ = tx.send(Event::Started);
        let started = Instant::now();
        loop {
            let finished = {
                let state = state.lock().unwrap();
                state.error.is_some()
                    || state.limits.should_stop(
                        stop.load(Ordering::Relaxed),
                        started.elapsed(),
                        state.frames,
                    )
            };
            if finished {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        wait_capture(&stream, false)
    })();
    // Taking the writer under the same mutex makes late callbacks harmless.
    let mut state = state.lock().map_err(|_| "Audio worker failed")?;
    let finalize = state
        .writer
        .take()
        .unwrap()
        .finalize()
        .map_err(|e| e.to_string());
    if let Err(error) = result
        .and(finalize)
        .and(state.error.take().map_or(Ok(()), Err))
    {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    if state.frames == 0 {
        let _ = std::fs::remove_file(&path);
        return Err("No audio received. Play sound in the selected browser and try again.".into());
    }
    let warning = if state.frames >= WAV_MAX_FRAMES {
        " — saved at the WAV file size limit (about 6 hours); start a new recording to continue."
    } else if state.peak < 0.0001 {
        " — audio was silent; check the source/tab."
    } else {
        ""
    };
    Ok((path, warning.into()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_request_rechecks_policy_when_source_changes() {
        let daw = "com.example.DAW";
        let browser = "com.apple.Safari";
        assert_eq!(recording_duration(daw, "invalid", true).unwrap(), None);
        assert!(recording_duration(daw, "invalid", false).is_err());
        assert_eq!(recording_duration(daw, "3601", false).unwrap(), Some(3601));
        assert_eq!(
            recording_duration(browser, "3601", false).is_ok(),
            recording_policy::EXTENDED_RECORDING
        );
        assert_eq!(
            recording_duration(browser, "30", true).is_ok(),
            recording_policy::EXTENDED_RECORDING
        );
        assert_eq!(
            recording_duration(browser, "1:00", false).unwrap(),
            Some(60)
        );
        assert!(recording_duration(browser, "invalid", false).is_err());
        assert!(recording_duration(browser, "0", false).is_err());
        assert!(recording_duration(daw, "3601", false).is_ok());
    }
    #[test]
    fn duration_validation() {
        for (input, expected) in [
            (" 30 ", 30),
            ("475", 475),
            ("7:55", 475),
            ("07:55", 475),
            (" 0:30 ", 30),
            ("0:01", 1),
            ("59:59", 3599),
            ("60:00", 3600),
            ("3600", 3600),
            ("3601", 3601),
            ("60:01", 3601),
            ("18446744073709551615", u64::MAX),
        ] {
            assert_eq!(parse_duration(input).unwrap(), expected, "{input}");
        }
        for input in [
            "0",
            "-1",
            "1.5",
            "abc",
            "",
            "0:00",
            "1:60",
            "1:5",
            ":30",
            "1:",
            "1:02:03",
            "1: 30",
            "+1:00",
            "18446744073709551615:59",
            "18446744073709551616",
        ] {
            assert!(parse_duration(input).is_err());
        }
    }
    #[test]
    fn pcm_layout_rejects_oversized_or_incompatible_frames() {
        let mut format = AudioStreamBasicDescription {
            mSampleRate: 48000.,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: 8,
            mFramesPerPacket: 1,
            mBytesPerFrame: 8,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        assert!(supported_pcm_format(&format));
        format.mBytesPerFrame = 16;
        assert!(!supported_pcm_format(&format));
        format.mFormatFlags |= kAudioFormatFlagIsNonInterleaved;
        format.mBytesPerFrame = 4;
        assert!(supported_pcm_format(&format));
        format.mBytesPerFrame = 8;
        assert!(!supported_pcm_format(&format));
        format.mBytesPerFrame = 4;
        format.mFormatFlags &= !kAudioFormatFlagIsPacked;
        assert!(!supported_pcm_format(&format));
        format.mFormatFlags |= kAudioFormatFlagIsPacked | kAudioFormatFlagIsBigEndian;
        assert!(!supported_pcm_format(&format));
    }
    #[test]
    fn stereo_channels_keep_order() {
        assert_eq!(
            interleave(&[0.1, 0.2], &[0.3, 0.4]),
            vec![0.1, 0.3, 0.2, 0.4]
        );
    }
    #[test]
    fn pcm_conversion_clips_and_handles_nan() {
        assert_eq!(quantize(2.), 32767);
        assert_eq!(quantize(-2.), -32767);
        assert_eq!(quantize(f32::NAN), 0);
        assert_eq!(quantize(0.), 0);
    }
}
