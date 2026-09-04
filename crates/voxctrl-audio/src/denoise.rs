//! RNNoise noise suppression for the capture path, behind the `noisereduce`
//! cargo feature.
//!
//! RNNoise is fixed to 48 kHz and works on 480-sample frames of 16-bit-scaled
//! PCM, none of which matches what the microphone hands us: the hardware runs
//! at whatever rate it negotiated, cpal delivers arbitrarily sized buffers of
//! `[-1.0, 1.0]` floats, and inference wants 16 kHz. [`Denoiser`] absorbs all
//! three mismatches — feed it a chunk at the hardware rate and it returns the
//! cleaned audio already resampled to [`TARGET_SAMPLE_RATE`], so it drops into
//! the capture callback in place of the plain resample.
//!
//! Samples that do not fill a whole 480-sample frame are held back and
//! prepended to the next chunk, so a frame is never zero-padded (which RNNoise
//! hears as a click). At most one frame — 10 ms — is therefore still buffered
//! when a recording ends.

#[cfg(feature = "noisereduce")]
use crate::{resample_chunk, TARGET_SAMPLE_RATE};

/// The only rate RNNoise understands.
#[cfg(feature = "noisereduce")]
const RNNOISE_SAMPLE_RATE: u32 = 48_000;

/// RNNoise wants `f32`s that came from 16-bit integers, i.e. the
/// `[-32768.0, 32767.0]` range rather than cpal's `[-1.0, 1.0]`.
#[cfg(feature = "noisereduce")]
const I16_SCALE: f32 = 32_768.0;

/// A stateful RNNoise denoiser for one capture stream.
///
/// RNNoise carries state between frames, so each stream needs its own — and a
/// rebuilt stream must start a fresh one rather than continuing with the
/// spectral estimate of the device it just left.
#[cfg(feature = "noisereduce")]
pub struct Denoiser {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    /// Sample rate of the chunks handed to [`Denoiser::process`].
    input_rate: u32,
    /// 48 kHz samples in i16 scale that did not fill a frame last time.
    pending: Vec<f32>,
    /// The first frame out of RNNoise contains fade-in artifacts.
    discard_first: bool,
}

/// Without the `noisereduce` feature there is no denoiser to construct, so the
/// type is uninhabited and every `Option<Denoiser>` is `None`.
#[cfg(not(feature = "noisereduce"))]
pub enum Denoiser {}

#[cfg(feature = "noisereduce")]
impl Denoiser {
    fn new(input_rate: u32) -> Self {
        Self {
            state: nnnoiseless::DenoiseState::new(),
            input_rate,
            pending: Vec::new(),
            discard_first: true,
        }
    }

    /// Denoise one capture chunk (at the hardware rate) and return it at
    /// [`TARGET_SAMPLE_RATE`].
    ///
    /// The result is shorter or longer than a plain resample of `input` would
    /// be, because whole frames are what RNNoise consumes; the caller must not
    /// assume a fixed ratio between input and output length.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        const FRAME: usize = nnnoiseless::DenoiseState::FRAME_SIZE;

        let at_48k = resample_chunk(input, self.input_rate, RNNOISE_SAMPLE_RATE);
        self.pending
            .extend(at_48k.iter().map(|s| s * I16_SCALE));

        let mut cleaned = Vec::with_capacity(self.pending.len());
        let mut frame = [0.0f32; FRAME];
        let mut out = [0.0f32; FRAME];
        let mut consumed = 0;

        while self.pending.len() - consumed >= FRAME {
            // Copy the frame out first: `process_frame` borrows `self.state`
            // mutably, so it cannot also hold a slice of `self.pending`.
            frame.copy_from_slice(&self.pending[consumed..consumed + FRAME]);
            self.state.process_frame(&mut out, &frame);
            consumed += FRAME;

            if self.discard_first {
                self.discard_first = false;
                continue;
            }
            cleaned.extend(out.iter().map(|s| s / I16_SCALE));
        }
        self.pending.drain(..consumed);

        resample_chunk(&cleaned, RNNOISE_SAMPLE_RATE, TARGET_SAMPLE_RATE)
    }
}

#[cfg(not(feature = "noisereduce"))]
impl Denoiser {
    pub fn process(&mut self, _input: &[f32]) -> Vec<f32> {
        match *self {}
    }
}

/// Build a denoiser for a stream capturing at `input_rate`, or `None` when
/// noise suppression is off or this build has no RNNoise compiled in.
pub fn make_denoiser(enabled: bool, input_rate: u32) -> Option<Denoiser> {
    if !enabled {
        return None;
    }

    #[cfg(feature = "noisereduce")]
    {
        tracing::info!("Noise suppression on: RNNoise denoising capture at {input_rate} Hz");
        Some(Denoiser::new(input_rate))
    }

    #[cfg(not(feature = "noisereduce"))]
    {
        let _ = input_rate;
        tracing::warn!(
            "Noise suppression is enabled in settings, but this build was compiled \
             without the `noisereduce` feature — capture audio is passed through unchanged"
        );
        None
    }
}

#[cfg(all(test, feature = "noisereduce"))]
mod tests {
    use super::*;

    /// Deterministic white noise in `[-0.5, 0.5]`, so the test cannot flake.
    fn noise(samples: usize) -> Vec<f32> {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        (0..samples)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                ((seed >> 40) as f32 / 16_777_216.0) - 0.5
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// The point of RNNoise is discrimination, not blanket attenuation: a
    /// steady tone (a stand-in for voiced speech) must come through intact
    /// while broadband noise is pulled down. Both halves matter — a denoiser
    /// fed samples at the wrong scale still "attenuates", it just wrecks the
    /// signal too.
    #[test]
    fn keeps_tone_and_attenuates_noise() {
        let seconds = 3;
        let noise_in: Vec<f32> = noise(48_000 * seconds).iter().map(|s| s * 0.1).collect();
        let noise_out = Denoiser::new(48_000).process(&noise_in);
        let noise_ratio = rms(&noise_out) / rms(&noise_in);

        let tone_in: Vec<f32> = (0..48_000 * seconds)
            .map(|i| (i as f32 * std::f32::consts::TAU * 440.0 / 48_000.0).sin() * 0.3)
            .collect();
        let tone_out = Denoiser::new(48_000).process(&tone_in);
        let tone_ratio = rms(&tone_out) / rms(&tone_in);

        assert!(tone_ratio > 0.9, "tone was mangled: kept {tone_ratio:.3} of it");
        assert!(noise_ratio < 0.95, "noise was not attenuated: kept {noise_ratio:.3}");
        assert!(
            noise_ratio < tone_ratio * 0.9,
            "denoiser did not tell noise ({noise_ratio:.3}) from tone ({tone_ratio:.3})"
        );
    }

    #[test]
    fn silence_stays_silent() {
        let out = Denoiser::new(48_000).process(&vec![0.0; 48_000]);
        assert!(out.iter().all(|s| s.abs() < 1e-6), "silence picked up noise");
    }

    #[test]
    fn resamples_to_target_rate() {
        // 44.1 kHz in, one second of it; out must be ~16 kHz worth of samples.
        let mut d = Denoiser::new(44_100);
        let out = d.process(&noise(44_100));
        let expected = TARGET_SAMPLE_RATE as usize;
        let slack = expected / 20; // 5%: frame alignment plus the discarded first frame
        assert!(
            out.len().abs_diff(expected) < slack,
            "expected ~{expected} samples at 16 kHz, got {}",
            out.len()
        );
    }

    #[test]
    fn carries_partial_frames_between_chunks() {
        // 100-sample chunks never line up with RNNoise's 480-sample frame, so
        // this only produces output at all if the remainder is carried over.
        let mut d = Denoiser::new(48_000);
        let input = noise(4_800);
        let produced: usize = input.chunks(100).map(|c| d.process(c).len()).sum();
        assert!(produced > 0, "chunked input produced no output at all");

        let mut whole = Denoiser::new(48_000);
        let at_once = whole.process(&input).len();
        assert!(
            produced.abs_diff(at_once) <= 160,
            "chunked ({produced}) and single-shot ({at_once}) output lengths diverged"
        );
    }

    #[test]
    fn disabled_builds_no_denoiser() {
        assert!(make_denoiser(false, 48_000).is_none());
        assert!(make_denoiser(true, 48_000).is_some());
    }
}
