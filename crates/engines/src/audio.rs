//! WAV decoding and sample-rate conversion into the mono 16 kHz f32 the engines expect.

use std::path::Path;

use anyhow::{Context, Result, bail};

use openflow_core::engine::SAMPLE_RATE;

/// Half-width of the windowed-sinc kernel, in source samples.
const TAPS: isize = 16;

/// Reads any PCM16/PCM24/PCM32/f32 WAV (any channel count, any rate) as mono 16 kHz f32.
pub fn load_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = reader.spec();

    let interleaved: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .with_context(|| format!("decoding {}", path.display()))?,
        (hound::SampleFormat::Int, bits @ (8 | 16 | 24 | 32)) => {
            let scale = 1.0 / (1u64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("decoding {}", path.display()))?
                .into_iter()
                .map(|s| s as f32 * scale)
                .collect()
        }
        (format, bits) => bail!("unsupported WAV: {format:?} with {bits} bits per sample"),
    };

    Ok(resample(&to_mono(&interleaved, spec.channels), spec.sample_rate, SAMPLE_RATE))
}

/// Averages interleaved frames down to a single channel. A trailing partial frame is dropped.
pub fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        return interleaved.to_vec();
    }
    let scale = 1.0 / channels as f32;
    interleaved.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() * scale).collect()
}

/// Windowed-sinc resampler. Used both for file decoding and for microphone capture.
pub fn resample(samples: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if samples.is_empty() || from_hz == 0 || to_hz == 0 || from_hz == to_hz {
        return samples.to_vec();
    }
    let ratio = to_hz as f64 / from_hz as f64;
    // Scale the sinc cutoff when downsampling so there is no aliasing.
    let cutoff = ratio.min(1.0);
    let out_len = (samples.len() as f64 * ratio).round() as usize;
    let last = samples.len() as isize - 1;

    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 / ratio;
        let center = pos.floor() as isize;
        let (mut acc, mut weight_sum) = (0.0f64, 0.0f64);
        for k in -TAPS..=TAPS {
            let index = center + k;
            if index < 0 || index > last {
                continue;
            }
            let distance = pos - index as f64;
            let weight = cutoff * sinc(cutoff * distance) * blackman(k as f64 / TAPS as f64);
            acc += f64::from(samples[index as usize]) * weight;
            weight_sum += weight;
        }
        out.push(if weight_sum.abs() > 1e-12 { (acc / weight_sum) as f32 } else { 0.0 });
    }
    out
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let pi_x = std::f64::consts::PI * x;
        pi_x.sin() / pi_x
    }
}

/// Blackman window over `u` in [-1, 1]; zero at the edges.
fn blackman(u: f64) -> f64 {
    0.42 + 0.5 * (std::f64::consts::PI * u).cos() + 0.08 * (2.0 * std::f64::consts::PI * u).cos()
}
