use std::collections::VecDeque;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

/// Peak level of each source over the last chunk, 0.0 to 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Levels {
    pub mic: f32,
    pub system: f32,
}

/// Audio held back before a source starts playing, so bursty delivery never runs dry.
const PREFILL_MS: usize = 30;
/// If a source ever holds more than this, the excess is dropped so audio cannot drift behind the video.
const MAX_LATENCY_MS: usize = 150;
/// What is kept when that happens.
const TRIM_TO_MS: usize = 50;

const fn frames(ms: usize) -> usize {
    SAMPLE_RATE as usize * ms / 1000
}

/// One input (microphone or system sound) with its own small jitter buffer.
#[derive(Debug)]
struct Lane {
    queue: VecDeque<f32>,
    primed: bool,
    prefill: usize,
}

impl Lane {
    fn new(prefill_frames: usize) -> Self {
        Self { queue: VecDeque::new(), primed: prefill_frames == 0, prefill: prefill_frames }
    }

    fn push(&mut self, samples: &[f32]) {
        self.queue.extend(samples);
        // A device clock running slightly fast would make the backlog grow for ever; trim it.
        if self.queue.len() > frames(MAX_LATENCY_MS) * CHANNELS {
            let excess = self.queue.len() - frames(TRIM_TO_MS) * CHANNELS;
            self.queue.drain(..excess - excess % CHANNELS);
        }
    }

    /// Fills `out` (interleaved stereo). Returns false if the lane had to be padded with silence.
    fn pull_into(&mut self, out: &mut [f32]) -> bool {
        if !self.primed && self.queue.len() >= self.prefill * CHANNELS {
            self.primed = true;
        }
        if !self.primed {
            out.fill(0.0);
            return false;
        }
        let mut ok = true;
        for slot in out.iter_mut() {
            match self.queue.pop_front() {
                Some(s) => *slot = s,
                None => {
                    *slot = 0.0;
                    ok = false;
                }
            }
        }
        if !ok {
            // Ran dry: wait for a cushion again instead of stuttering on every packet.
            self.primed = false;
        }
        ok
    }
}

/// Mixes the microphone and system sound into one stereo 48 kHz stream.
///
/// Devices deliver audio in irregular bursts, and system loopback delivers nothing at all
/// during silence. So the mixer is *pulled* on a steady clock: every pull returns exactly the
/// requested number of frames. Each source keeps a short cushion (the jitter buffer) so a late
/// packet does not turn into a click, and pads with silence only when a source truly has nothing.
#[derive(Debug)]
pub struct Mixer {
    system: Lane,
    mic: Lane,
    pub system_gain: f32,
    pub mic_gain: f32,
}

impl Mixer {
    pub fn new(system_gain: f32, mic_gain: f32) -> Self {
        Self::with_prefill(system_gain, mic_gain, frames(PREFILL_MS))
    }

    /// `prefill_frames` is how much each source must buffer before it plays.
    pub fn with_prefill(system_gain: f32, mic_gain: f32, prefill_frames: usize) -> Self {
        Self { system: Lane::new(prefill_frames), mic: Lane::new(prefill_frames), system_gain, mic_gain }
    }

    /// Interleaved stereo f32 at 48 kHz.
    pub fn push_system(&mut self, samples: &[f32]) {
        self.system.push(samples);
    }

    pub fn push_mic(&mut self, samples: &[f32]) {
        self.mic.push(samples);
    }

    /// Produces `frames` stereo frames as little-endian signed 16-bit PCM, plus the levels.
    pub fn pull(&mut self, frames: usize) -> (Vec<u8>, Levels) {
        let n = frames * CHANNELS;
        let mut sys = vec![0.0f32; n];
        let mut mic = vec![0.0f32; n];
        self.system.pull_into(&mut sys);
        self.mic.pull_into(&mut mic);

        let mut pcm = Vec::with_capacity(n * 2);
        let (mut peak_mic, mut peak_sys) = (0.0f32, 0.0f32);
        for (s, m) in sys.iter().zip(&mic) {
            let (s, m) = (s * self.system_gain, m * self.mic_gain);
            peak_sys = peak_sys.max(s.abs());
            peak_mic = peak_mic.max(m.abs());
            pcm.extend_from_slice(&((soft_clip(s + m) * i16::MAX as f32) as i16).to_le_bytes());
        }
        (pcm, Levels { mic: peak_mic.min(1.0), system: peak_sys.min(1.0) })
    }

    pub fn clear(&mut self) {
        for lane in [&mut self.system, &mut self.mic] {
            lane.queue.clear();
            lane.primed = lane.prefill == 0;
        }
    }

    #[cfg(test)]
    fn buffered(&self) -> (usize, usize) {
        (self.system.queue.len(), self.mic.queue.len())
    }
}

/// Transparent below 0.8, then smoothly compresses toward 1.0 so two loud sources never wrap or crackle.
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let over = (a - KNEE) / (1.0 - KNEE);
        x.signum() * (KNEE + (1.0 - KNEE) * over.tanh())
    }
}

/// Converts mono samples to interleaved stereo.
pub fn mono_to_stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter().flat_map(|&s| [s, s]).collect()
}

/// Converts an interleaved buffer with any channel count to interleaved stereo.
pub fn to_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 => Vec::new(),
        1 => mono_to_stereo(samples),
        2 => samples.to_vec(),
        n => samples.chunks_exact(n).flat_map(|f| [f[0], f[1]]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(pcm: &[u8]) -> Vec<i16> {
        pcm.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
    }

    /// A mixer with no cushion, for tests that check the mixing itself.
    fn plain(system_gain: f32, mic_gain: f32) -> Mixer {
        Mixer::with_prefill(system_gain, mic_gain, 0)
    }

    #[test]
    fn pull_always_returns_the_requested_length_and_pads_with_silence() {
        let mut m = Mixer::new(1.0, 1.0);
        let (pcm, levels) = m.pull(480);
        assert_eq!(pcm.len(), 480 * 2 * 2);
        assert!(samples(&pcm).iter().all(|&s| s == 0));
        assert_eq!(levels, Levels::default());
    }

    #[test]
    fn short_input_is_followed_by_silence_not_stale_data() {
        let mut m = plain(1.0, 1.0);
        m.push_system(&[0.5; 4]);
        let s = samples(&m.pull(4).0);
        assert!(s[..4].iter().all(|&v| v > 10_000) && s[4..].iter().all(|&v| v == 0));
    }

    #[test]
    fn sources_are_summed_and_levels_reported_per_source() {
        let mut m = plain(1.0, 1.0);
        m.push_system(&[0.2; 4]);
        m.push_mic(&[0.3; 4]);
        let (pcm, levels) = m.pull(2);
        let expected = (0.5 * i16::MAX as f32) as i16;
        assert!(samples(&pcm).iter().all(|&s| (s - expected).abs() < 3));
        assert!((levels.system - 0.2).abs() < 1e-6 && (levels.mic - 0.3).abs() < 1e-6);
    }

    #[test]
    fn loud_mix_is_limited_without_wrapping() {
        let mut m = plain(1.0, 1.0);
        m.push_system(&[1.0; 8]);
        m.push_mic(&[1.0; 8]);
        for s in samples(&m.pull(4).0) {
            assert!(s > 30_000, "no wraparound to negative, got {s}");
        }
    }

    #[test]
    fn gain_scales_each_source() {
        let mut m = plain(0.5, 0.0);
        m.push_system(&[0.8; 2]);
        m.push_mic(&[0.8; 2]);
        let (pcm, levels) = m.pull(1);
        let expected = (0.4 * i16::MAX as f32) as i16;
        assert!(samples(&pcm).iter().all(|&s| (s - expected).abs() < 3));
        assert_eq!(levels.mic, 0.0);
    }

    #[test]
    fn a_stalled_consumer_cannot_grow_the_buffer_forever() {
        let mut m = Mixer::new(1.0, 1.0);
        for _ in 0..100 {
            m.push_mic(&vec![0.1; 48_000 * 2]);
        }
        let (_, mic) = m.buffered();
        assert!(mic <= frames(MAX_LATENCY_MS) * CHANNELS);
        assert_eq!(mic % CHANNELS, 0, "never split a stereo frame");
    }

    /// Simulates what really happens: the device hands over a fixed 10 ms packet every 10 ms, while
    /// the consumer thread wakes up at uneven intervals (4 to 16 ms) and asks for exactly the time
    /// that has passed. Returns how many pulls (after warm-up) contained a silent sample.
    fn uneven_wakeups_gaps(mut m: Mixer) -> usize {
        let mut seed = 99u32;
        let mut rand = |n: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % n
        };
        let (mut next_pull, mut last_pull, mut gaps) = (10u32, 0u32, 0);
        for ms in 0..4_000u32 {
            if ms >= 3 && (ms - 3) % 10 == 0 {
                m.push_mic(&vec![0.3; frames(10) * CHANNELS]);
            }
            if ms == next_pull {
                let (pcm, _) = m.pull((ms - last_pull) as usize * 48);
                if ms > 300 && samples(&pcm).contains(&0) {
                    gaps += 1;
                }
                last_pull = ms;
                next_pull = ms + 4 + rand(13);
            }
        }
        gaps
    }

    #[test]
    fn uneven_wakeups_play_without_gaps_with_the_jitter_buffer() {
        assert_eq!(uneven_wakeups_gaps(Mixer::new(1.0, 1.0)), 0);
    }

    #[test]
    fn running_dry_waits_for_a_cushion_instead_of_stuttering() {
        let mut m = Mixer::with_prefill(1.0, 1.0, 100);
        m.push_mic(&vec![0.5; 100 * CHANNELS]);
        assert!(samples(&m.pull(50).0).iter().any(|&s| s > 0), "primed, so it plays");
        // Only 50 frames left, asked for 80: pads, then re-primes.
        let s = samples(&m.pull(80).0);
        assert!(s[..100].iter().all(|&v| v > 0) && s[100..].iter().all(|&v| v == 0));
        m.push_mic(&vec![0.5; 40 * CHANNELS]);
        assert!(samples(&m.pull(40).0).iter().all(|&s| s == 0), "below the cushion, stays quiet");
        m.push_mic(&vec![0.5; 80 * CHANNELS]);
        assert!(samples(&m.pull(40).0).iter().all(|&s| s > 0), "cushion reached, plays again");
    }

    #[test]
    fn clear_forgets_buffered_sound() {
        let mut m = plain(1.0, 1.0);
        m.push_mic(&[0.5; 100]);
        m.clear();
        assert!(samples(&m.pull(10).0).iter().all(|&s| s == 0));
    }

    #[test]
    fn channel_conversion() {
        assert_eq!(mono_to_stereo(&[0.1, 0.2]), vec![0.1, 0.1, 0.2, 0.2]);
        assert_eq!(to_stereo(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3), vec![1.0, 2.0, 4.0, 5.0]);
        assert_eq!(to_stereo(&[1.0, 2.0], 2), vec![1.0, 2.0]);
        assert!(to_stereo(&[1.0], 0).is_empty());
    }
}
