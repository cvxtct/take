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
