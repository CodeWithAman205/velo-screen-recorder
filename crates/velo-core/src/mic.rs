use nnnoiseless::DenoiseState;
use std::collections::VecDeque;

const FRAME: usize = DenoiseState::FRAME_SIZE; // 480 samples = 10 ms at 48 kHz
/// RNNoise works on samples scaled like 16-bit integers.
const SCALE: f32 = 32_768.0;

/// Cleans up microphone audio: removes rumble below the voice, then suppresses steady background
/// noise (fans, hiss, keyboard hum) with RNNoise. Takes and returns interleaved stereo at 48 kHz.
///
/// RNNoise needs whole 10 ms frames, so output trails the input by up to 10 ms. The mixer's
/// jitter buffer absorbs that.
pub struct MicProcessor {
    denoise: Option<Box<DenoiseState<'static>>>,
    high_pass: HighPass,
    pending: Vec<f32>,
    ready: VecDeque<f32>,
    primed: bool,
}

impl MicProcessor {
    /// With `denoise` false only the rumble filter runs.
    pub fn new(denoise: bool) -> Self {
        Self {
            denoise: denoise.then(DenoiseState::new),
            high_pass: HighPass::new(80.0, 48_000.0),
            pending: Vec::with_capacity(FRAME),
            ready: VecDeque::new(),
            primed: false,
        }
    }

    pub fn process(&mut self, stereo: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(stereo.len());
        for frame in stereo.as_chunks::<2>().0 {
            let mono = self.high_pass.process((frame[0] + frame[1]) * 0.5);
            let Some(denoise) = self.denoise.as_mut() else {
                out.extend([mono, mono]);
                continue;
            };
            self.pending.push(mono * SCALE);
            if self.pending.len() == FRAME {
                let mut cleaned = [0.0f32; FRAME];
                denoise.process_frame(&mut cleaned, &self.pending);
                self.pending.clear();
                // The very first frame has fade-in artefacts, so replace it with silence.
                let discard = !self.primed;
                self.primed = true;
                self.ready.extend(cleaned.iter().map(|s| if discard { 0.0 } else { s / SCALE }));
            }
            if let Some(s) = self.ready.pop_front() {
                out.extend([s, s]);
            }
        }
        out
    }
}

/// Second-order Butterworth high-pass: keeps the voice, removes desk thumps and DC offset.
struct HighPass {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl HighPass {
    fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let (cos, sin) = (w.cos(), w.sin());
        let alpha = sin / std::f32::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        Self {
            b: [(1.0 + cos) / 2.0 / a0, -(1.0 + cos) / a0, (1.0 + cos) / 2.0 / a0],
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Deterministic white noise, interleaved stereo.
    fn noise(frames: usize, amplitude: f32) -> Vec<f32> {
        let mut seed = 7u32;
        (0..frames)
            .flat_map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let v = ((seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0) * amplitude;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn steady_background_noise_is_strongly_reduced() {
        let mut p = MicProcessor::new(true);
        let input = noise(48_000, 0.05);
        let output = p.process(&input);
        let (i, o) = (rms(&input[48_000..]), rms(&output[output.len() - 48_000..]));
        assert!(o < i * 0.5, "noise should at least halve (6 dB): in {i}, out {o}");
    }

    #[test]
    fn output_length_tracks_input_within_one_frame() {
        let mut p = MicProcessor::new(true);
        let mut total_in = 0;
        let mut total_out = 0;
        for chunk in [441, 480, 1000, 17, 960] {
            let input = noise(chunk, 0.1);
            total_in += input.len();
            total_out += p.process(&input).len();
        }
        assert!(total_in - total_out <= FRAME * 2, "lag is at most one frame: {total_in} vs {total_out}");
    }

    #[test]
    fn output_is_always_stereo_with_equal_channels() {
        let mut p = MicProcessor::new(true);
        let out = p.process(&noise(2_000, 0.1));
        assert_eq!(out.len() % 2, 0);
        assert!(out.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]));
    }

    #[test]
    fn rumble_filter_removes_dc_and_keeps_the_voice_band() {
        let mut p = MicProcessor::new(false);
        let dc: Vec<f32> = vec![0.3; 2 * 48_000];
        let out = p.process(&dc);
        assert!(out[out.len() - 100..].iter().all(|s| s.abs() < 0.01), "constant offset removed");

        let mut p = MicProcessor::new(false);
        let tone: Vec<f32> = (0..48_000).flat_map(|n| {
            let v = 0.3 * (2.0 * std::f32::consts::PI * 1_000.0 * n as f32 / 48_000.0).sin();
            [v, v]
        }).collect();
        let out = p.process(&tone);
        assert!(rms(&out[out.len() / 2..]) > rms(&tone[tone.len() / 2..]) * 0.95, "1 kHz passes");
    }

    #[test]
    fn silence_stays_silent() {
        let mut p = MicProcessor::new(true);
        let out = p.process(&vec![0.0; 2 * 4_800]);
        assert!(out.iter().all(|s| s.abs() < 1e-4));
    }

    #[test]
    fn disabled_denoise_does_not_delay_audio() {
        let mut p = MicProcessor::new(false);
        assert_eq!(p.process(&noise(333, 0.1)).len(), 666);
    }
}
