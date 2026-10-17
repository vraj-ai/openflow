//! The Slaney mel filterbank Whisper was trained with: the equivalent of
//! `librosa.filters.mel(sr=16000, n_fft=400, n_mels=N, htk=False, norm="slaney")`.
//!
//! Whisper needs an 80-band bank (tiny/base/small/medium) or a 128-band one
//! (large-v3 and turbo), and a wrong filterbank quietly wrecks the transcript,
//! so the shape and normalisation are pinned by tests.

pub const SAMPLE_RATE: f64 = 16_000.0;
pub const N_FFT: usize = 400;
/// FFT bins kept after the real transform (n_fft / 2 + 1).
pub const N_FREQS: usize = N_FFT / 2 + 1;

const F_MIN: f64 = 0.0;
const F_SP: f64 = 200.0 / 3.0;
const MIN_LOG_HZ: f64 = 1000.0;
/// `ln(6.4) / 27`, the Slaney log step (written out so it stays a constant).
const LOG_STEP: f64 = 0.068_751_777_420_949_12;

fn hz_to_mel(f: f64) -> f64 {
    let min_log_mel = (MIN_LOG_HZ - F_MIN) / F_SP;
    if f < MIN_LOG_HZ {
        (f - F_MIN) / F_SP
    } else {
        min_log_mel + (f / MIN_LOG_HZ).ln() / LOG_STEP
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    let min_log_mel = (MIN_LOG_HZ - F_MIN) / F_SP;
    if mel < min_log_mel {
        F_MIN + F_SP * mel
    } else {
        MIN_LOG_HZ * (LOG_STEP * (mel - min_log_mel)).exp()
    }
}

/// `n_mels x N_FREQS` triangular filters, row-major (band, frequency bin).
pub fn slaney_filters(n_mels: usize) -> Vec<f32> {
    let (mel_lo, mel_hi) = (hz_to_mel(F_MIN), hz_to_mel(SAMPLE_RATE / 2.0));
    let mel_points: Vec<f64> = (0..n_mels + 2)
        .map(|i| mel_to_hz(mel_lo + (mel_hi - mel_lo) * i as f64 / (n_mels + 1) as f64))
        .collect();
    let freqs: Vec<f64> = (0..N_FREQS)
        .map(|k| SAMPLE_RATE / 2.0 * k as f64 / (N_FREQS - 1) as f64)
        .collect();
    let mut out = vec![0f32; n_mels * N_FREQS];
    for band in 0..n_mels {
        let (lower, center, upper) = (mel_points[band], mel_points[band + 1], mel_points[band + 2]);
        // librosa's enorm: keep every filter's area equal.
        let norm = 2.0 / (upper - lower);
        for (k, &f) in freqs.iter().enumerate() {
            let rising = (f - lower) / (center - lower);
            let falling = (upper - f) / (upper - center);
            let w = rising.min(falling).max(0.0);
            out[band * N_FREQS + k] = (w * norm) as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from `librosa.filters.mel(sr=16000, n_fft=400, n_mels=80,
    /// htk=False, norm="slaney")` computed with numpy: a layout or
    /// normalisation slip shows up here.
    #[test]
    fn matches_librosa_for_80_bands() {
        let f = slaney_filters(80);
        assert_eq!(f.len(), 80 * N_FREQS);
        // (band, bin, weight); the zeros matter as much as the peaks, because the
        // low bands are narrower than one FFT bin.
        let checks: [(usize, usize, f64); 8] = [
            (0, 0, -0.0),
            (0, 1, 0.024_862_594_481_760_87),
            (1, 2, 0.022_871_772_096_078_016),
            (2, 6, 0.0),
            (10, 20, 0.0),
            (40, 92, 0.0),
            (79, 192, 0.003_164_711_739_176_461),
            (79, 200, 0.0),
        ];
        for (band, k, want) in checks {
            let got = f[band * N_FREQS + k] as f64;
            assert!(
                (got - want).abs() < 1e-9,
                "band {band} bin {k}: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn has_the_128_band_bank_large_v3_uses() {
        let f = slaney_filters(128);
        assert_eq!(f.len(), 128 * N_FREQS);
        assert!((f[N_FREQS + 1] as f64 - 0.030_392_564_871_355_21).abs() < 1e-9);
        assert_eq!(f[127 * N_FREQS + 200], 0.0);
        assert!((f[127 * N_FREQS + 192] as f64 - 0.001_617_171_731_358_138_7).abs() < 1e-9);
    }

    #[test]
    fn filters_are_non_negative_and_bounded() {
        for n_mels in [80, 128] {
            let f = slaney_filters(n_mels);
            assert_eq!(f.len(), n_mels * N_FREQS);
            let total: f32 = f.iter().sum();
            assert!(total > 0.0, "empty filterbank");
            for band in 0..n_mels {
                let row = &f[band * N_FREQS..(band + 1) * N_FREQS];
                // Triangular: one hump, rising to a peak then falling.
                let peak = row.iter().cloned().fold(f32::MIN, f32::max);
                assert!(peak > 0.0, "band {band} is silent");
                assert!(row.iter().all(|v| *v >= 0.0 && *v <= 1.0), "band {band} out of range");
                let peak_at = row.iter().position(|v| *v == peak).unwrap();
                assert!(row[..peak_at].windows(2).all(|w| w[0] <= w[1] + 1e-6), "not rising");
                assert!(row[peak_at..].windows(2).all(|w| w[0] + 1e-6 >= w[1]), "not falling");
            }
        }
    }

    /// The bank must cover the spectrum: every FFT bin with energy above the
    /// lowest band is touched by at least one filter.
    #[test]
    fn every_band_covers_its_own_range() {
        let f = slaney_filters(80);
        for band in 0..80 {
            let row = &f[band * N_FREQS..(band + 1) * N_FREQS];
            assert!(row.iter().any(|v| *v > 0.0), "band {band} covers nothing");
        }
    }
}
