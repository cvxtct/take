use std::{
    ffi::CString,
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

unsafe extern "C" {
    fn renamex_np(
        from: *const std::ffi::c_char,
        to: *const std::ffi::c_char,
        flags: u32,
    ) -> std::ffi::c_int;
}

/// Rename a recording within its current directory, without replacing any file.
pub fn rename_clip(source: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    let stem = if name.to_ascii_lowercase().ends_with(".wav") {
        &name[..name.len() - 4]
    } else {
        name
    };
    if stem.trim().is_empty()
        || matches!(stem, "." | "..")
        || name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':') || c.is_control())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Enter a filename without path separators, colons, or control characters.",
        ));
    }
    let destination = source.with_file_name(format!("{stem}.wav"));
    if !std::fs::symlink_metadata(source)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Select a regular recording file.",
        ));
    }
    if source == destination {
        return Ok(destination);
    }
    let from = CString::new(source.as_os_str().as_bytes())?;
    let to = CString::new(destination.as_os_str().as_bytes())?;
    // macOS sys/stdio.h: RENAME_EXCL = 0x00000004; fail if destination exists.
    // SAFETY (#4): Both pointers reference live NUL-terminated CStrings for this
    // synchronous call. The signature and flag match the macOS SDK; no pointers escape.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let result = unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 0x00000004) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct RecordingDir(tempfile::TempDir);
    impl RecordingDir {
        fn new() -> Self {
            Self(tempfile::tempdir().unwrap())
        }
        fn recording(&self, name: &str) -> PathBuf {
            let path = self.0.path().join(name);
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 48000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&path, spec).unwrap();
            writer.write_sample(1234_i16).unwrap();
            writer.finalize().unwrap();
            path
        }
    }
    #[test]
    fn renames_recording_preserving_audio_and_wav_extension() {
        let dir = RecordingDir::new();
        let source = dir.recording("Rhythm-123.wav");
        let renamed = rename_clip(&source, "Dub chord").unwrap();
        assert_eq!(renamed, dir.0.path().join("Dub chord.wav"));
        assert!(!source.exists());
        let mut audio = hound::WavReader::open(renamed).unwrap();
        assert_eq!(audio.samples::<i16>().next().unwrap().unwrap(), 1234);
    }
    #[test]
    fn collision_preserves_both_recordings() {
        let dir = RecordingDir::new();
        let source = dir.recording("source.wav");
        let existing = dir.0.path().join("existing.wav");
        fs::write(&existing, b"existing recording").unwrap();
        assert_eq!(
            rename_clip(&source, "existing").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(hound::WavReader::open(source).is_ok());
        assert_eq!(fs::read(existing).unwrap(), b"existing recording");
    }

    #[test]
    fn invalid_names_leave_original_untouched() {
        let dir = RecordingDir::new();
        let source = dir.recording("source.wav");
        for name in [
            "",
            "   ",
            ".",
            "..",
            ".wav",
            "../escape",
            "sub/name",
            "sub\\name",
            "bad\0name",
            "bad\nname",
            "bad:name",
        ] {
            assert_eq!(
                rename_clip(&source, name).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{name:?}"
            );
            assert!(hound::WavReader::open(&source).is_ok());
        }
    }

    #[test]
    fn accepts_optional_extension_unicode_and_unchanged_name() {
        let dir = RecordingDir::new();
        let source = dir.recording("source.wav");
        let renamed = rename_clip(&source, "  Mély akkord.WAV  ").unwrap();
        assert_eq!(renamed, dir.0.path().join("Mély akkord.wav"));
        assert_eq!(rename_clip(&renamed, "Mély akkord.wav").unwrap(), renamed);
        assert!(hound::WavReader::open(renamed).is_ok());
    }

    #[test]
    fn refuses_directories_and_symlinks_without_moving_them() {
        let dir = RecordingDir::new();
        let folder = dir.0.path().join("folder.wav");
        fs::create_dir(&folder).unwrap();
        assert!(rename_clip(&folder, "moved").is_err());
        assert!(folder.is_dir());
        let source = dir.recording("source.wav");
        let link = dir.0.path().join("link.wav");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert!(rename_clip(&link, "moved").is_err());
        assert!(link.is_symlink());
        assert!(source.exists());
    }

    #[test]
    fn filesystem_failures_do_not_create_or_replace_files() {
        let dir = RecordingDir::new();
        let missing = dir.0.path().join("missing.wav");
        assert_eq!(
            rename_clip(&missing, "new").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!dir.0.path().join("new.wav").exists());
        let source = dir.recording("source.wav");
        assert!(rename_clip(&source, &"x".repeat(300)).is_err());
        assert!(hound::WavReader::open(&source).is_ok());
        let link = dir.0.path().join("occupied.wav");
        std::os::unix::fs::symlink(&missing, &link).unwrap();
        assert_eq!(
            rename_clip(&source, "occupied").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(source.exists());
        assert!(link.is_symlink());
    }
}
