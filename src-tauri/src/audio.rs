use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const TARGET_RATE: u32 = 16000;

fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == 0 || from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let i0 = src_pos.floor() as usize;
        let i1 = (i0 + 1).min(input.len().saturating_sub(1));
        let frac = src_pos - i0 as f64;
        let s = input[i0] as f64 * (1.0 - frac) + input[i1] as f64 * frac;
        out.push(s as f32);
    }
    out
}

fn decode_buffer_ref(buf: AudioBufferRef<'_>) -> Result<Vec<f32>, String> {
    let spec = *buf.spec();
    let dur = buf.capacity() as u64;
    let mut sample_buf = SampleBuffer::<f32>::new(dur, spec);
    sample_buf.copy_interleaved_ref(buf);
    let all = sample_buf.samples();
    let channels = spec.channels.count();
    if channels == 0 {
        return Err("Keine Audiokanäle".to_string());
    }
    if channels == 1 {
        return Ok(all.to_vec());
    }
    let frames = all.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum = 0f32;
        for c in 0..channels {
            sum += all[f * channels + c];
        }
        mono.push(sum / channels as f32);
    }
    Ok(mono)
}

/// Liest Audio mit Symphonia und liefert mono f32 @ 16 kHz für whisper.cpp
pub fn decode_file_to_mono_16k(path: &Path) -> Result<Vec<f32>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let src = MediaSourceStream::new(Box::new(file), Default::default());
    let mss = symphonia::default::get_probe()
        .format(&hint, src, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| e.to_string())?;

    let mut format = mss.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some()
        })
        .ok_or_else(|| "Kein nutzbarer Audio-Track".to_string())?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "Unbekannte Sample-Rate".to_string())?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;

    let track_id = track.id;
    let mut samples_mono: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphError::ResetRequired) => continue,
            Err(SymphError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let chunk = decode_buffer_ref(decoded)?;
                samples_mono.extend_from_slice(&chunk);
            }
            Err(SymphError::DecodeError(_)) => continue,
            Err(SymphError::IoError(_)) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    if samples_mono.is_empty() {
        return Err("Keine Audiodaten erkannt".to_string());
    }

    Ok(resample_linear(&samples_mono, sample_rate, TARGET_RATE))
}
