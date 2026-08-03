use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const TARGET_RATE: u32 = 16000;

/// Anti-alias cutoff, a little under the 8 kHz output Nyquist to leave the
/// filter a transition band. Speech energy above this is mostly fricative
/// detail, which matters far less than keeping folded content out of the band.
const ANTIALIAS_CUTOFF_HZ: f64 = 7200.0;

/// Butterworth Q values for a 6th-order response built from three biquads.
/// Fourth order only reached about -23 dB at 12 kHz, which still folds audibly
/// into the speech band; this gets roughly -35 dB there.
const BUTTERWORTH_Q: [f64; 3] = [0.517_638_1, std::f64::consts::FRAC_1_SQRT_2, 1.931_851_7];

/// Direct-form-1 biquad.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// Low-pass section (RBJ cookbook).
    fn low_pass(sample_rate: f64, cutoff: f64, q: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);

        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 - cos_w0) / 2.0) / a0,
            b1: (1.0 - cos_w0) / a0,
            b2: ((1.0 - cos_w0) / 2.0) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x0: f64) -> f64 {
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        y0
    }
}

/// Streaming mono resampler to 16 kHz.
///
/// Two things matter here. It consumes packets as they are decoded, so only the
/// 16 kHz result is held rather than the full-rate signal plus a resampled copy
/// (a three-hour 48 kHz episode used to need well over 2 GB). And it low-passes
/// before decimating: plain interpolation folded everything above 8 kHz back
/// into the audible band, which degrades what Whisper receives.
struct Resampler {
    ratio: f64,
    filter: Vec<Biquad>,
    /// Filtered input not yet consumed, plus the fractional read position in it.
    pending: Vec<f32>,
    pos: f64,
    out: Vec<f32>,
}

impl Resampler {
    fn new(from_rate: u32, to_rate: u32) -> Self {
        let from = f64::from(from_rate);
        let to = f64::from(to_rate);
        // Only downsampling aliases; upsampling needs no guard filter.
        let filter = if from_rate > to_rate {
            BUTTERWORTH_Q
                .iter()
                .map(|&q| Biquad::low_pass(from, ANTIALIAS_CUTOFF_HZ, q))
                .collect()
        } else {
            Vec::new()
        };

        Self {
            ratio: from / to,
            filter,
            pending: Vec::new(),
            pos: 0.0,
            out: Vec::new(),
        }
    }

    fn push(&mut self, chunk: &[f32]) {
        self.pending.reserve(chunk.len());
        for &s in chunk {
            let mut v = f64::from(s);
            for section in &mut self.filter {
                v = section.process(v);
            }
            self.pending.push(v as f32);
        }
        self.drain();
    }

    fn drain(&mut self) {
        // Needs pos and pos+1 in range to interpolate.
        while self.pos + 1.0 < self.pending.len() as f64 {
            let i0 = self.pos as usize;
            let frac = self.pos - i0 as f64;
            let a = f64::from(self.pending[i0]);
            let b = f64::from(self.pending[i0 + 1]);
            self.out.push((a + (b - a) * frac) as f32);
            self.pos += self.ratio;
        }

        // Drop input the read position has moved past.
        let consumed = self.pos.floor().max(0.0) as usize;
        if consumed > 0 {
            let consumed = consumed.min(self.pending.len());
            self.pending.drain(..consumed);
            self.pos -= consumed as f64;
        }
    }

    fn finish(mut self) -> Vec<f32> {
        // Emit the final sample so a short clip is not dropped entirely.
        if let Some(&last) = self.pending.last() {
            if self.pos < self.pending.len() as f64 {
                self.out.push(last);
            }
        }
        self.out
    }
}

/// Downmixes an interleaved buffer to mono, reusing `mono` to avoid allocating
/// per packet (a long file decodes into hundreds of thousands of packets).
fn downmix_into(samples: &[f32], channels: usize, mono: &mut Vec<f32>) -> Result<(), String> {
    if channels == 0 {
        return Err("No audio channels".to_string());
    }
    mono.clear();
    if channels == 1 {
        mono.extend_from_slice(samples);
        return Ok(());
    }
    let frames = samples.len() / channels;
    mono.reserve(frames);
    for f in 0..frames {
        let mut sum = 0f32;
        for c in 0..channels {
            sum += samples[f * channels + c];
        }
        mono.push(sum / channels as f32);
    }
    Ok(())
}

/// Reads audio with Symphonia and returns mono f32 @ 16 kHz for whisper.cpp.
pub fn decode_file_to_mono_16k(path: &Path) -> Result<Vec<f32>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let src = MediaSourceStream::new(Box::new(file), Default::default());
    let mss = symphonia::default::get_probe()
        .format(
            &hint,
            src,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;

    let mut format = mss.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| "No usable audio track".to_string())?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "Unknown sample rate".to_string())?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;

    let track_id = track.id;
    let mut resampler = Resampler::new(sample_rate, TARGET_RATE);
    let mut sample_buf: Option<(SampleBuffer<f32>, SignalSpec, u64)> = None;
    let mut mono: Vec<f32> = Vec::new();
    let mut decoded_any = false;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // The demuxer reset (chained streams); the decoder must follow suit.
            Err(SymphError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(SymphError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let capacity = decoded.capacity() as u64;
                // Allocated once and reused; recreated only if the stream changes shape.
                let needs_new = match &sample_buf {
                    Some((_, s, c)) => *s != spec || *c < capacity,
                    None => true,
                };
                if needs_new {
                    sample_buf = Some((SampleBuffer::<f32>::new(capacity, spec), spec, capacity));
                }
                let (buf, _, _) = sample_buf.as_mut().expect("just populated above");
                buf.copy_interleaved_ref(decoded);

                downmix_into(buf.samples(), spec.channels.count(), &mut mono)?;
                resampler.push(&mono);
                decoded_any = true;
            }
            // A corrupt packet here and there is recoverable; skip just that one.
            Err(SymphError::DecodeError(_)) => continue,
            // Only a clean end of stream may stop decoding. Treating every I/O
            // error as EOF silently produced a half transcript that was then
            // written out as a success.
            Err(SymphError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    if !decoded_any {
        return Err("No audio data detected".to_string());
    }

    let samples = resampler.finish();
    if samples.is_empty() {
        return Err("No audio data detected".to_string());
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::{decode_file_to_mono_16k, downmix_into, Resampler, TARGET_RATE};
    use std::io::Write;

    /// Minimal 16-bit PCM WAV, so the decode path can be exercised without a fixture.
    fn write_wav(path: &std::path::Path, channels: u16, rate: u32, frames: &[Vec<f32>]) {
        let bits = 16u16;
        let block_align = channels * bits / 8;
        let byte_rate = rate * u32::from(block_align);
        let data_len = (frames.len() * usize::from(block_align)) as u32;

        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes()); // PCM
        w.extend_from_slice(&channels.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&byte_rate.to_le_bytes());
        w.extend_from_slice(&block_align.to_le_bytes());
        w.extend_from_slice(&bits.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        for frame in frames {
            for s in frame {
                let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                w.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::File::create(path)
            .expect("create wav")
            .write_all(&w)
            .expect("write wav");
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("voxmd-audio-test-{name}.wav"));
        p
    }

    #[test]
    fn decodes_a_real_file_to_16k_mono() {
        let path = temp_path("stereo48k");
        let src_rate = 48_000u32;
        let secs = 0.5;
        let frames: Vec<Vec<f32>> = (0..(f64::from(src_rate) * secs) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(src_rate);
                let v = (2.0 * std::f64::consts::PI * 440.0 * t).sin() as f32;
                vec![v, v]
            })
            .collect();
        write_wav(&path, 2, src_rate, &frames);

        let out = decode_file_to_mono_16k(&path).expect("decode");
        let _ = std::fs::remove_file(&path);

        let expected = (f64::from(TARGET_RATE) * secs) as usize;
        assert!(
            (out.len() as i64 - expected as i64).abs() < 50,
            "expected ~{expected} samples at 16 kHz, got {}",
            out.len()
        );
        assert!(
            rms(&out) > 0.5,
            "440 Hz tone should survive, rms={}",
            rms(&out)
        );
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(decode_file_to_mono_16k(std::path::Path::new("/no/such/file.wav")).is_err());
    }

    fn resample(input: &[f32], from: u32) -> Vec<f32> {
        let mut r = Resampler::new(from, TARGET_RATE);
        // Pushed in chunks so the test also covers state carried across packets.
        for chunk in input.chunks(97) {
            r.push(chunk);
        }
        r.finish()
    }

    fn sine(freq: f64, rate: u32, secs: f64) -> Vec<f32> {
        let n = (f64::from(rate) * secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    fn rms(xs: &[f32]) -> f64 {
        if xs.is_empty() {
            return 0.0;
        }
        (xs.iter()
            .map(|x| f64::from(*x) * f64::from(*x))
            .sum::<f64>()
            / xs.len() as f64)
            .sqrt()
    }

    #[test]
    fn same_rate_is_passthrough_length() {
        let input = sine(440.0, TARGET_RATE, 0.1);
        let out = resample(&input, TARGET_RATE);
        // Linear interpolation at ratio 1.0 reproduces the input, give or take
        // the final sample.
        assert!((out.len() as i64 - input.len() as i64).abs() <= 1);
    }

    #[test]
    fn downsampling_produces_the_expected_length() {
        let input = sine(440.0, 48_000, 0.5);
        let out = resample(&input, 48_000);
        let expected = (input.len() as f64 * f64::from(TARGET_RATE) / 48_000.0) as usize;
        let drift = (out.len() as i64 - expected as i64).abs();
        assert!(
            drift <= 2,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }

    /// The regression this filter exists for: without a low-pass, a 12 kHz tone
    /// sampled at 48 kHz folds down to 4 kHz and lands right in the speech band.
    #[test]
    fn content_above_output_nyquist_is_attenuated() {
        let audible = resample(&sine(1_000.0, 48_000, 0.5), 48_000);
        let aliasing = resample(&sine(12_000.0, 48_000, 0.5), 48_000);

        let kept = rms(&audible);
        let folded = rms(&aliasing);
        assert!(kept > 0.5, "1 kHz tone should pass through, rms={kept}");
        assert!(
            folded < kept / 30.0,
            "12 kHz tone should be filtered out before decimation, but rms={folded} vs {kept}"
        );
    }

    #[test]
    fn low_frequency_content_survives_resampling() {
        let out = resample(&sine(300.0, 44_100, 0.3), 44_100);
        assert!(rms(&out) > 0.5, "300 Hz tone was lost, rms={}", rms(&out));
    }

    #[test]
    fn upsampling_needs_no_filter_and_grows() {
        let input = sine(1_000.0, 8_000, 0.2);
        let out = resample(&input, 8_000);
        assert!(out.len() > input.len());
        assert!(rms(&out) > 0.5);
    }

    #[test]
    fn stereo_is_averaged_to_mono() {
        let mut mono = Vec::new();
        // Interleaved L/R: (1,-1) cancels, (0.5,0.5) stays.
        downmix_into(&[1.0, -1.0, 0.5, 0.5], 2, &mut mono).unwrap();
        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn mono_passes_through_downmix() {
        let mut buf = Vec::new();
        downmix_into(&[0.1, 0.2, 0.3], 1, &mut buf).unwrap();
        assert_eq!(buf, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn zero_channels_is_an_error() {
        let mut buf = Vec::new();
        assert!(downmix_into(&[0.1], 0, &mut buf).is_err());
    }

    #[test]
    fn empty_input_yields_no_samples() {
        assert!(resample(&[], 48_000).is_empty());
    }

    /// The reused downmix buffer must not leak samples between packets.
    #[test]
    fn downmix_buffer_is_reset_between_calls() {
        let mut buf = Vec::new();
        downmix_into(&[1.0, 1.0, 1.0, 1.0], 2, &mut buf).unwrap();
        assert_eq!(buf.len(), 2);
        downmix_into(&[0.5, 0.5], 2, &mut buf).unwrap();
        assert_eq!(buf, vec![0.5]);
    }
}
