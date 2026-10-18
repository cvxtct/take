# Native-binding review (issue #16)

Semgrep's unsafe-usage rule identifies review sites, not proven vulnerabilities.
Reviewed sites have a SAFETY explanation and a suppression for that exact rule
on the adjacent expression. No file or rule is excluded globally: additional
unsafe sites and other security patterns remain blocking. Re-review a site when
its code or the assumptions below change.

## Ownership and threading

AppKit objects and Delegate/Canvas are main-thread-only. Delegate is retained
through the application run loop; controls, window and timer are retained.
Private target/action call sites use the matching one-object-argument methods;
menu actions use standard responder-chain signatures. NSObject/NSView initializers
match their superclass signatures and are called after ivar initialization.
Protocol implementations inherit NSObject behavior and use the declared Apple
callback signatures. Audio callbacks do not manipulate AppKit objects.

Shareable-content callback objects are retained before returning. NSError
pointers are read only within Apple's callback lifetime and copied to owned
strings. The raw-pointer helper is now unsafe so callers must establish this
contract. Copied completion blocks own channel senders, including after timeout.

## Audio memory

AudioBufferList storage uses repr(C), with space for two buffers. Float vectors
are aligned and alive throughout the synchronous CoreMedia copy. Capture accepts
only 48 kHz, stereo, packed native little-endian float32 with matching bytes per
frame (4 planar, 8 interleaved). Frames are bounded before allocating or narrowing
the count; data is copied and no borrowed PCM pointer escapes the callback.
The review added packed/bytes-per-frame validation and regression tests.

The output object owns Arc<Mutex<AudioState>>; writes and writer finalization use
the same mutex. Late callbacks cannot write after the writer is taken. Stream,
filter, output and queue remain retained while start/stop is awaited. Timeout
and OS errors are propagated; only audio output is registered. No raw pointer
is sent to the UI.

## Limits

This is a source-level review, not a formal proof or a substitute for testing
capture on macOS. It relies on Apple and objc2 ownership/callback contracts.
Changes to callback lifetimes, delegate retention, buffer formats or selectors
require renewed review. Semgrep does not validate these contracts for us.

## Executable path

The current-exe rule also flags the default recording folder. This call only
suggests an editable output path beside the app; it is not an identity or trust
check and never loads or executes code. Files are created with create_new under
the user's existing privileges. This one call has a rule-specific exception.

## Exclusive recording rename (issue #4)

`clips::rename_clip` calls macOS `renamex_np` with `RENAME_EXCL` (0x00000004,
from the SDK's `sys/stdio.h`). The filesystem rejects an existing destination
atomically; no check-then-overwrite fallback is used. CString storage remains
alive throughout the synchronous C call, and OS errors are returned to the UI.
Names cannot contain path separators or control characters. Source files must
be regular files; directories and symlinks are rejected before the operation.

## Source policy (issue #31)

Capture reads the selected application's immutable bundle identifier from its
retained ScreenCaptureKit snapshot before creating a WAV. The same pure policy
used by the UI validates the request again in the capture worker. The source
popup's selectSource: target is the retained main-thread Delegate.

CaptureLimits bounds both timed and manual PCM writes to the RIFF ceiling.
Duration-to-frame arithmetic saturates before applying that ceiling. Elapsed
time is compared to the requested duration without constructing a potentially
overflowing Instant deadline. No permission reset is performed by this feature.
