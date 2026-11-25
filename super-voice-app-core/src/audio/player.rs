// SPDX-License-Identifier: GPL-3.0-only
//! Playing a speech clip on the default output device, for the Clips tab.
//!
//! The one sound the app makes itself: a clip is a file the app keeps, and
//! playing it is the app's, not the daemon's. Each play opens the device on a
//! thread of its own (a cpal stream is `!Send` on several backends), plays the
//! clip once, and closes it. Stopping, or dropping the [`Player`], ends it at
//! once.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// What the playing thread and the app both see.
#[derive(Debug, Default)]
struct Shared {
    /// Frames handed to the device so far, at the device's rate.
    played: AtomicUsize,
    /// The whole clip, in frames at the device's rate; `0` until the device
    /// is open.
    total: AtomicUsize,
    /// The device's rate; `0` until it is open.
    rate: AtomicUsize,
    /// Every frame has been handed to the device, or it failed.
    finished: AtomicBool,
    stop: AtomicBool,
    error: Mutex<Option<String>>,
}

/// A clip playing. Dropping it stops it.
pub struct Player {
    shared: Arc<Shared>,
    length: Duration,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

impl Player {
    /// Start playing mono `samples` at `rate`.
    #[must_use]
    pub fn start(samples: Vec<f32>, rate: u32) -> Self {
        let length = duration_of(samples.len(), rate);
        let shared = Arc::new(Shared::default());
        let thread = std::thread::Builder::new()
            .name("clip-player".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || play(&shared, &samples, rate)
            })
            .ok();
        if thread.is_none() {
            fail(&shared, "could not start playing".to_string());
        }
        Self {
            shared,
            length,
            thread,
        }
    }

    /// A player that plays nothing and opens no device, held `position` into
    /// a clip `length` long: what an app's screenshot draws as a clip playing,
    /// with no sound on the desktop it is taken on.
    #[must_use]
    pub fn held_at(length: Duration, position: Duration) -> Self {
        const RATE: u32 = 1_000;
        let shared = Arc::new(Shared::default());
        shared.rate.store(RATE as usize, Ordering::Relaxed);
        #[allow(clippy::cast_possible_truncation)]
        let played = position.min(length).as_millis() as usize;
        shared.played.store(played, Ordering::Relaxed);
        Self {
            shared,
            length,
            thread: None,
        }
    }

    /// How long the clip is.
    #[must_use]
    pub const fn length(&self) -> Duration {
        self.length
    }

    /// How far it has played.
    #[must_use]
    pub fn position(&self) -> Duration {
        let rate = self.shared.rate.load(Ordering::Relaxed);
        let rate = u32::try_from(rate).unwrap_or(0);
        duration_of(self.shared.played.load(Ordering::Relaxed), rate).min(self.length)
    }

    /// Whether it has played to the end, or failed.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }

    /// Why it could not play, if it could not.
    #[must_use]
    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().ok().and_then(|e| e.clone())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn duration_of(frames: usize, rate: u32) -> Duration {
    if rate == 0 {
        return Duration::ZERO;
    }
    #[allow(clippy::cast_precision_loss)]
    Duration::from_secs_f64(frames as f64 / f64::from(rate))
}

fn fail(shared: &Shared, why: String) {
    if let Ok(mut error) = shared.error.lock() {
        *error = Some(why);
    }
    shared.finished.store(true, Ordering::Relaxed);
}

/// The playing thread: open the device, play, wait for the end or a stop.
fn play(shared: &Arc<Shared>, samples: &[f32], rate: u32) {
    let stream = match open(shared, samples, rate) {
        Ok(stream) => stream,
        Err(e) => {
            fail(shared, e);
            return;
        }
    };
    if let Err(e) = stream.play() {
        fail(shared, format!("could not start the speakers: {e}"));
        return;
    }
    while !shared.stop.load(Ordering::Relaxed) && !shared.finished.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(20));
    }
    // The last block handed over is still in the device's buffer; let it
    // sound before the stream goes, unless the user stopped it.
    if !shared.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(150));
    }
    drop(stream);
}

fn open(shared: &Arc<Shared>, samples: &[f32], rate: u32) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no speakers were found".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("the speakers offered no usable format: {e}"))?;
    let config = supported.config();
    let channels = usize::from(config.channels).max(1);
    let device_rate = config.sample_rate;
    let audio = Arc::new(resample(samples, rate, device_rate));
    shared.total.store(audio.len(), Ordering::Relaxed);
    shared
        .rate
        .store(usize::try_from(device_rate).unwrap_or(0), Ordering::Relaxed);

    let err_shared = Arc::clone(shared);
    let err_fn = move |e: cpal::Error| {
        if super::stream_goes_on(&e) {
            log::debug!("The speakers glitched, and play on: {e}");
        } else {
            fail(&err_shared, format!("the speakers stopped: {e}"));
        }
    };

    match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let shared = Arc::clone(shared);
            device.build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill(&shared, &audio, data, channels, |s| s);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let shared = Arc::clone(shared);
            device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    fill(&shared, &audio, data, channels, to_i16);
                },
                err_fn,
                None,
            )
        }
        other => return Err(format!("unsupported speaker sample format: {other}")),
    }
    .map_err(|e| format!("could not open the speakers: {e}"))
}

#[allow(clippy::cast_possible_truncation)]
fn to_i16(sample: f32) -> i16 {
    (sample * f32::from(i16::MAX)).clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

/// Hand the device its next block: each mono sample on every channel, then
/// silence once the clip is over.
fn fill<T: Copy + Default>(
    shared: &Shared,
    audio: &[f32],
    data: &mut [T],
    channels: usize,
    convert: impl Fn(f32) -> T,
) {
    if shared.stop.load(Ordering::Relaxed) {
        data.fill(T::default());
        return;
    }
    let start = shared.played.load(Ordering::Relaxed);
    let mut frame = start;
    for out in data.chunks_mut(channels) {
        let value = audio.get(frame).map_or(T::default(), |s| convert(*s));
        out.fill(value);
        if frame < audio.len() {
            frame += 1;
        }
    }
    shared.played.store(frame, Ordering::Relaxed);
    if frame >= audio.len() {
        shared.finished.store(true, Ordering::Relaxed);
    }
}

/// `samples` at `from` Hz, at `to` Hz, by straight-line interpolation: plenty
/// for speech, which carries nothing near the top of either band.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || to == 0 || samples.is_empty() {
        return samples.to_vec();
    }
    let step = f64::from(from) / f64::from(to);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let frames = (samples.len() as f64 / step).floor() as usize;
    (0..frames)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let at = i as f64 * step;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let index = at.floor() as usize;
            #[allow(clippy::cast_possible_truncation)]
            let fraction = (at - at.floor()) as f32;
            let here = samples[index];
            let next = samples.get(index + 1).copied().unwrap_or(here);
            here + (next - here) * fraction
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_rate_is_left_alone() {
        assert_eq!(resample(&[0.1, 0.2, 0.3], 24_000, 24_000), [0.1, 0.2, 0.3]);
    }

    #[test]
    fn doubling_the_rate_doubles_the_frames_and_fills_between() {
        let out = resample(&[0.0, 1.0, 0.0, -1.0], 24_000, 48_000);
        assert_eq!(out.len(), 8);
        assert!((out[1] - 0.5).abs() < 1e-6, "{out:?}");
        assert!((out[2] - 1.0).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn a_block_puts_each_sample_on_every_channel_then_silence() {
        let shared = Shared::default();
        let audio = [0.5, -0.5];
        let mut block = [9.0_f32; 6];

        fill(&shared, &audio, &mut block, 2, |s| s);

        assert_eq!(block, [0.5, 0.5, -0.5, -0.5, 0.0, 0.0]);
        assert!(shared.finished.load(Ordering::Relaxed));
        assert_eq!(shared.played.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_stopped_clip_hands_over_silence() {
        let shared = Shared::default();
        shared.stop.store(true, Ordering::Relaxed);
        let mut block = [9.0_f32; 4];

        fill(&shared, &[0.5; 10], &mut block, 1, |s| s);

        assert_eq!(block, [0.0; 4]);
        assert_eq!(shared.played.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_held_player_stays_where_it_was_put() {
        let player = Player::held_at(Duration::from_secs(6), Duration::from_secs(2));
        assert_eq!(player.position(), Duration::from_secs(2));
        assert_eq!(player.length(), Duration::from_secs(6));
        assert!(!player.finished());
    }

    #[test]
    fn a_clip_is_as_long_as_its_frames_at_its_rate() {
        assert_eq!(duration_of(36_000, 24_000), Duration::from_millis(1_500));
        assert_eq!(duration_of(10, 0), Duration::ZERO);
    }
}
