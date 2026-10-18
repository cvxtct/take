//! Source-aware recording policy, shared by the UI and capture worker.
pub const EXTENDED_RECORDING: bool = cfg!(feature = "extended-recording");

pub fn is_browser(bundle: &str) -> bool {
    [
        "com.apple.Safari",
        "com.google.Chrome",
        "org.mozilla.firefox",
        "com.microsoft.edgemac",
        "company.thebrowser.Browser",
        "com.brave.Browser",
    ]
    .contains(&bundle)
}

pub fn browser_limited(bundle: &str) -> bool {
    !EXTENDED_RECORDING && is_browser(bundle)
}

pub fn validate_duration(bundle: &str, duration: Option<u64>) -> Result<(), String> {
    if duration == Some(0) {
        return Err("Recording duration must be greater than zero.".into());
    }
    if browser_limited(bundle) && !duration.is_some_and(|s| s <= 60) {
        return Err("Browser recordings are limited to 1–60 seconds in this build.".into());
    }
    Ok(())
}

// Leave room for WAV headers below the RIFF 32-bit size limit.
pub const WAV_MAX_FRAMES: u64 = (u32::MAX as u64 - 1024) / 4;

pub struct CaptureLimits {
    pub max_frames: u64,
    duration: Option<u64>,
}
impl CaptureLimits {
    pub fn new(bundle: &str, duration: Option<u64>) -> Result<Self, String> {
        validate_duration(bundle, duration)?;
        Ok(Self {
            max_frames: duration.map_or(WAV_MAX_FRAMES, |s| {
                s.saturating_mul(48000).min(WAV_MAX_FRAMES)
            }),
            duration,
        })
    }
    pub fn should_stop(&self, stop: bool, elapsed: std::time::Duration, frames: u64) -> bool {
        stop || frames >= self.max_frames || self.duration.is_some_and(|s| elapsed.as_secs() >= s)
    }
    pub fn samples_to_write(&self, frames: u64, available: usize) -> usize {
        available.min((self.max_frames.saturating_sub(frames) * 2) as usize) / 2 * 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_policy_matches_the_compiled_build() {
        assert!(validate_duration("com.apple.Safari", Some(1)).is_ok());
        assert!(validate_duration("com.apple.Safari", Some(60)).is_ok());
        assert_eq!(
            validate_duration("com.apple.Safari", Some(61)).is_ok(),
            EXTENDED_RECORDING
        );
        assert_eq!(
            validate_duration("com.apple.Safari", None).is_ok(),
            EXTENDED_RECORDING
        );
        assert!(validate_duration("com.apple.Safari", Some(0)).is_err());
    }
    #[test]
    fn capture_limits_bound_large_and_manual_recordings_safely() {
        for duration in [None, Some(100_000), Some(u64::MAX)] {
            let limits = CaptureLimits::new("com.example.DAW", duration).unwrap();
            assert_eq!(limits.max_frames, 1_073_741_567);
            assert_eq!(limits.samples_to_write(limits.max_frames - 1, 200), 2);
            assert!(limits.should_stop(false, std::time::Duration::ZERO, limits.max_frames));
            assert_eq!(limits.samples_to_write(limits.max_frames, 200), 0);
        }
    }

    #[test]
    fn capture_policy_uses_source_identity_and_enforces_frame_boundary() {
        for bundle in [
            "com.apple.Safari",
            "com.google.Chrome",
            "org.mozilla.firefox",
            "com.microsoft.edgemac",
            "company.thebrowser.Browser",
            "com.brave.Browser",
        ] {
            assert!(is_browser(bundle));
            assert_eq!(CaptureLimits::new(bundle, None).is_ok(), EXTENDED_RECORDING);
            assert_eq!(
                CaptureLimits::new(bundle, Some(3601)).is_ok(),
                EXTENDED_RECORDING
            );
            let limits = CaptureLimits::new(bundle, Some(60)).unwrap();
            assert_eq!(limits.max_frames, 2_880_000);
            assert_eq!(limits.samples_to_write(2_879_999, 960), 2);
            assert!(!limits.should_stop(false, std::time::Duration::from_secs(59), 0));
            assert!(limits.should_stop(false, std::time::Duration::from_secs(60), 0));
            assert!(limits.should_stop(true, std::time::Duration::ZERO, 0));
        }
        for bundle in ["com.example.DAW", "com.example.UnknownBrowser"] {
            assert!(!is_browser(bundle));
            assert!(CaptureLimits::new(bundle, None).is_ok());
            assert!(CaptureLimits::new(bundle, Some(3601)).is_ok());
        }
        assert!(CaptureLimits::new("com.example.DAW", Some(0)).is_err());
    }
}
