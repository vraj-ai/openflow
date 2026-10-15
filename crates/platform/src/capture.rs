//! Microphone capture through cpal. Samples are kept at the device's native
//! rate and channel count; the engines crate converts them to 16 kHz mono.

use std::sync::{Arc, Mutex};

use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct Recording {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Opens the default input device once and records while `start`..`stop`.
/// The stream stays open between recordings so a key press starts capture
/// without device start-up latency.
pub struct Recorder {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Option<Vec<f32>>>>,
    sample_rate: u32,
    channels: u16,
}

impl Recorder {
    pub fn open_default() -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host.default_input_device().context("no default microphone")?;
        let config = device.default_input_config().context("microphone has no input config")?;
        let sample_rate = config.sample_rate();
        let channels = config.channels();
        let buffer: Arc<Mutex<Option<Vec<f32>>>> = Arc::default();
        let sink = buffer.clone();
        let format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let err = |e| log::error!("microphone stream error: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |data: &[f32], _| append(&sink, data.iter().copied()),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |data: &[i16], _| append(&sink, data.iter().map(|s| *s as f32 / 32768.0)),
                err,
                None,
            ),
            cpal::SampleFormat::I32 => device.build_input_stream(
                stream_config,
                move |data: &[i32], _| append(&sink, data.iter().map(|s| *s as f32 / 2147483648.0)),
                err,
                None,
            ),
            other => return Err(anyhow!("unsupported microphone sample format {other:?}")),
        }?;
        stream.play()?;
        Ok(Self { stream, buffer, sample_rate, channels })
    }

    pub fn start(&self) {
        *self.buffer.lock().unwrap() = Some(Vec::with_capacity(self.sample_rate as usize * 30));
    }

    pub fn stop(&self) -> Recording {
        let samples = self.buffer.lock().unwrap().take().unwrap_or_default();
        Recording { samples, sample_rate: self.sample_rate, channels: self.channels }
    }

    pub fn pause_device(&self) -> anyhow::Result<()> {
        Ok(self.stream.pause()?)
    }
}

fn append(sink: &Mutex<Option<Vec<f32>>>, data: impl Iterator<Item = f32>) {
    if let Some(buf) = sink.lock().unwrap().as_mut() {
        buf.extend(data);
    }
}
