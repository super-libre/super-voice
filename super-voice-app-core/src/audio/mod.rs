// SPDX-License-Identifier: GPL-3.0-only
//! The app's own audio: capturing a reference recording for a cloned voice,
//! and playing back the speech clips it keeps.
//!
//! Speech the user asks for goes through the daemon, which owns the output
//! for it. Two things the daemon cannot do on the app's behalf are done here.
//! Recording: the microphone is opened for as long as the user holds the
//! button and for nothing else, which is a property worth keeping visible in
//! the process the user is looking at. And playing a clip: a clip is a file
//! the app keeps, not something the daemon has, so the app plays it.

pub mod player;
pub mod recorder;

/// Whether a stream error leaves the stream running: a buffer under- or
/// overrun, a moment's glitch, or the default device changing under a stream
/// that follows it. Anything else has stopped the stream.
///
/// macOS reports an overrun as a clip starts on some Macs, and the clip
/// plays on; stopping for it made every clip fail to play there.
#[must_use]
pub(crate) fn stream_goes_on(error: &cpal::Error) -> bool {
    matches!(
        error.kind(),
        cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged
    )
}

#[cfg(test)]
mod tests {
    use super::stream_goes_on;

    #[test]
    fn a_glitch_or_a_new_default_device_is_not_the_end() {
        assert!(stream_goes_on(&cpal::Error::new(cpal::ErrorKind::Xrun)));
        assert!(stream_goes_on(&cpal::Error::new(
            cpal::ErrorKind::DeviceChanged
        )));
    }

    #[test]
    fn a_device_gone_is() {
        assert!(!stream_goes_on(&cpal::Error::new(
            cpal::ErrorKind::DeviceNotAvailable
        )));
    }
}
