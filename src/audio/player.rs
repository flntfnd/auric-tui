use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use rodio::cpal::Sample as CpalSample;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use tokio::sync::mpsc;

use super::spectrum::SharedSpectrum;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Playing,
    Paused,
    Resumed,
    Stopped,
    Error,
    VolumeChanged,
}

pub struct AudioPlayer {
    _stream: OutputStream,
    _stream_handle: OutputStreamHandle,
    sink: Sink,
    state: PlaybackState,
    volume: f32,
    current_path: Option<String>,
    position_secs: Arc<AtomicU64>,
    is_playing: Arc<AtomicBool>,
    duration: Duration,
    event_tx: Option<mpsc::Sender<PlayerEvent>>,
    spectrum: SharedSpectrum,
}

impl AudioPlayer {
    pub fn new() -> Result<Self> {
        let (stream, stream_handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&stream_handle)?;

        Ok(Self {
            _stream: stream,
            _stream_handle: stream_handle,
            sink,
            state: PlaybackState::Stopped,
            volume: 1.0,
            current_path: None,
            position_secs: Arc::new(AtomicU64::new(0)),
            is_playing: Arc::new(AtomicBool::new(false)),
            duration: Duration::ZERO,
            event_tx: None,
            spectrum: SharedSpectrum::new(),
        })
    }

    /// Get the shared spectrum for visualization
    pub fn spectrum(&self) -> &SharedSpectrum {
        &self.spectrum
    }

    fn send_event(&self, event: PlayerEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(event);
        }
    }

    pub fn play(&mut self, path: &Path, duration: Duration) -> Result<()> {
        // Stop any current playback
        self.stop();

        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let source = Decoder::new(reader)?;

        // Wrap source to track position and capture spectrum
        let position_secs = Arc::clone(&self.position_secs);
        let spectrum = self.spectrum.clone();

        let source = PositionTrackingSource::new(source, position_secs, spectrum);

        self.sink.append(source);
        self.sink.set_volume(self.volume);
        self.sink.play();

        self.duration = duration;
        self.state = PlaybackState::Playing;
        self.current_path = Some(path.display().to_string());
        self.is_playing.store(true, Ordering::SeqCst);

        self.send_event(PlayerEvent::Playing);

        Ok(())
    }

    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.sink.pause();
            self.state = PlaybackState::Paused;
            self.is_playing.store(false, Ordering::SeqCst);
            self.send_event(PlayerEvent::Paused);
        }
    }

    pub fn resume(&mut self) {
        if self.state == PlaybackState::Paused {
            self.sink.play();
            self.state = PlaybackState::Playing;
            self.is_playing.store(true, Ordering::SeqCst);
            self.send_event(PlayerEvent::Resumed);
        }
    }

    pub fn toggle_pause(&mut self) {
        match self.state {
            PlaybackState::Playing => self.pause(),
            PlaybackState::Paused => self.resume(),
            PlaybackState::Stopped => {}
        }
    }

    pub fn stop(&mut self) {
        self.sink.stop();
        // Create a new sink for future playback
        if let Ok(sink) = Sink::try_new(&self._stream_handle) {
            self.sink = sink;
        }
        self.state = PlaybackState::Stopped;
        self.current_path = None;
        self.position_secs.store(0, Ordering::SeqCst);
        self.is_playing.store(false, Ordering::SeqCst);
        self.duration = Duration::ZERO;
        self.spectrum.clear();
        self.send_event(PlayerEvent::Stopped);
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        if self.sink.try_seek(position).is_err() {
            self.send_event(PlayerEvent::Error);
        }
        Ok(())
    }

    pub fn seek_forward(&mut self, amount: Duration) -> Result<()> {
        let current = self.position();
        let new_pos = current + amount;
        if new_pos < self.duration {
            self.seek(new_pos)?;
        }
        Ok(())
    }

    pub fn seek_backward(&mut self, amount: Duration) -> Result<()> {
        let current = self.position();
        let new_pos = current.saturating_sub(amount);
        self.seek(new_pos)?;
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.sink.set_volume(self.volume);
        self.send_event(PlayerEvent::VolumeChanged);
    }

    pub fn volume_up(&mut self) {
        self.set_volume(self.volume + 0.05);
    }

    pub fn volume_down(&mut self) {
        self.set_volume(self.volume - 0.05);
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn position(&self) -> Duration {
        Duration::from_secs(self.position_secs.load(Ordering::SeqCst))
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn is_finished(&self) -> bool {
        self.sink.empty() && self.state == PlaybackState::Playing
    }

    pub fn progress(&self) -> f64 {
        if self.duration.as_secs() == 0 {
            0.0
        } else {
            self.position().as_secs_f64() / self.duration.as_secs_f64()
        }
    }
}

// Wrapper to track playback position and capture spectrum samples
struct PositionTrackingSource<S> {
    source: S,
    position_secs: Arc<AtomicU64>,
    samples_played: u64,
    sample_rate: u32,
    channels: u16,
    spectrum: SharedSpectrum,
    channel_idx: u16,
}

impl<S: Source> PositionTrackingSource<S>
where
    S::Item: rodio::Sample,
{
    fn new(
        source: S,
        position_secs: Arc<AtomicU64>,
        spectrum: SharedSpectrum,
    ) -> Self {
        let sample_rate = source.sample_rate();
        let channels = source.channels();
        Self {
            source,
            position_secs,
            samples_played: 0,
            sample_rate,
            channels,
            spectrum,
            channel_idx: 0,
        }
    }
}

impl<S: Source> Iterator for PositionTrackingSource<S>
where
    S::Item: rodio::Sample,
    f32: rodio::cpal::FromSample<S::Item>,
{
    type Item = S::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.source.next()?;

        self.samples_played += 1;

        // Push samples to spectrum analyzer (only first channel to save CPU)
        if self.channel_idx == 0 {
            // Convert sample to f32 for spectrum analysis
            let sample_f32 = sample.to_sample::<f32>();
            self.spectrum.push_sample(sample_f32);
        }

        // Track which channel we're on
        self.channel_idx = (self.channel_idx + 1) % self.channels;

        // Update position every second worth of samples
        let samples_per_second = self.sample_rate as u64 * self.channels as u64;
        if self.samples_played % samples_per_second == 0 {
            let secs = self.samples_played / samples_per_second;
            self.position_secs.store(secs, Ordering::SeqCst);
        }

        Some(sample)
    }
}

impl<S: Source> Source for PositionTrackingSource<S>
where
    S::Item: rodio::Sample,
    f32: rodio::cpal::FromSample<S::Item>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.source.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.source.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}
