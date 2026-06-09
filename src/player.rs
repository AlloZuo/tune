use std::io::Cursor;
use std::time::Duration;

use anyhow::Result;
use rand::seq::SliceRandom;
use rand::rng;
use rodio::{
    source::SeekError, Decoder, OutputStream, OutputStreamHandle, Sink, Source,
};

// ── Playback mode ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayMode {
    /// Play the list sequentially; stop after the last track.
    Sequential,
    /// Repeat the current track forever.
    SingleRepeat,
    /// Shuffle through the current list; re-shuffle when exhausted.
    Shuffle,
}

impl PlayMode {
    pub fn next_variant(self) -> Self {
        match self {
            PlayMode::Sequential => PlayMode::SingleRepeat,
            PlayMode::SingleRepeat => PlayMode::Shuffle,
            PlayMode::Shuffle => PlayMode::Sequential,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Sequential => "顺序播放",
            PlayMode::SingleRepeat => "单曲循环",
            PlayMode::Shuffle => "随机播放",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            PlayMode::Sequential => "顺序",
            PlayMode::SingleRepeat => "单曲",
            PlayMode::Shuffle => "随机",
        }
    }
}

// ── Shuffle state ──

#[derive(Debug, Clone)]
pub struct ShuffleState {
    remaining: Vec<usize>,
}

impl ShuffleState {
    /// Build a fresh shuffled queue of `count` items (indices 0..count-1).
    pub fn new(count: usize) -> Self {
        let mut indices: Vec<usize> = (0..count).collect();
        indices.shuffle(&mut rng());
        Self { remaining: indices }
    }

    /// Pop the next index; returns `None` if the queue is empty.
    pub fn next(&mut self) -> Option<usize> {
        self.remaining.pop()
    }

    /// Re-shuffle the whole set (used when the queue is exhausted).
    #[allow(dead_code)]
    pub fn reshuffle(&mut self, count: usize) {
        *self = Self::new(count);
    }
}

// ── Player state ──

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerState {
    Playing,
    Paused,
    Stopped,
}

use crate::lyrics::Lyrics;

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub total_duration_ms: u64,
    pub lyrics: Option<Lyrics>,
}

// ── Player ──

pub struct Player {
    sink: Option<Sink>,
    _stream: OutputStream,
    _stream_handle: OutputStreamHandle,
    state: PlayerState,
    current_track: Option<TrackInfo>,
    volume: f32,

    /// Stored raw audio bytes — needed for seeking.
    audio_data: Option<Vec<u8>>,
    /// Absolute position (ms) at which the current sink/decoder started.
    seek_offset_ms: u64,

    // Playback mode
    pub play_mode: PlayMode,
    /// `None` when not in shuffle mode.
    pub shuffle_state: Option<ShuffleState>,
}

impl Player {
    pub fn new() -> Result<Self> {
        let (_stream, stream_handle) = OutputStream::try_default()?;
        Ok(Self {
            sink: None,
            _stream,
            _stream_handle: stream_handle,
            state: PlayerState::Stopped,
            current_track: None,
            volume: 1.0,
            audio_data: None,
            seek_offset_ms: 0,
            play_mode: PlayMode::Sequential,
            shuffle_state: None,
        })
    }

    /// Load and play audio from raw bytes.
    pub fn play_bytes(&mut self, data: Vec<u8>, track: TrackInfo) -> Result<()> {
        self.audio_data = Some(data.clone());

        let cursor = Cursor::new(data);
        let source = Decoder::new(cursor)?;

        if let Some(sink) = &self.sink {
            sink.stop();
        }

        // Fill in the actual duration from the decoded audio when unknown.
        let mut track = track;
        if track.total_duration_ms == 0 {
            if let Some(dur) = source.total_duration() {
                track.total_duration_ms = dur.as_millis() as u64;
            }
        }

        let sink = Sink::try_new(&self._stream_handle)?;
        sink.set_volume(self.volume);
        sink.append(source);

        self.sink = Some(sink);
        self.seek_offset_ms = 0;
        self.state = PlayerState::Playing;
        self.current_track = Some(track);
        Ok(())
    }

    // ── Seek ──

    /// Seek to an absolute position in milliseconds.
    pub fn seek_to_ms(&mut self, pos_ms: u64) -> Result<()> {
        let data = self
            .audio_data
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("没有可 seek 的音频数据"))?;
        let total = self
            .current_track
            .as_ref()
            .map(|t| t.total_duration_ms)
            .unwrap_or(0);
        let pos_ms = pos_ms.min(total.saturating_sub(200)); // leave 200ms margin
        let seek_dur = Duration::from_millis(pos_ms);

        // Try native decoder seek first (works for MP3, WAV, etc.).
        // If the decoder doesn't support seeking (FLAC, etc.), fall back
        // to skip_duration which skips decoded samples manually.
        let cursor = Cursor::new(data.clone());
        let mut source = Decoder::new(cursor)?;

        let seeked_source: Box<dyn Source<Item = i16> + Send> = match source.try_seek(seek_dur) {
            Ok(()) => Box::new(source),
            Err(SeekError::NotSupported { .. }) => {
                // Re-decode and skip samples
                let cursor2 = Cursor::new(data.clone());
                let source2 = Decoder::new(cursor2)?;
                Box::new(source2.skip_duration(seek_dur))
            }
            Err(_) => anyhow::bail!("seek 失败"),
        };

        if let Some(sink) = &self.sink {
            sink.stop();
        }
        let sink = Sink::try_new(&self._stream_handle)?;
        sink.set_volume(self.volume);
        sink.append(seeked_source);

        self.sink = Some(sink);
        self.seek_offset_ms = pos_ms;
        self.state = PlayerState::Playing;
        Ok(())
    }

    /// Seek relative to current position (positive = forward, negative = backward).
    pub fn seek_relative(&mut self, delta_secs: i64) -> Result<()> {
        let cur = self.position_ms() as i64;
        let new = (cur + delta_secs * 1000).max(0) as u64;
        self.seek_to_ms(new)
    }

    // ── Playback control ──

    pub fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            if let Some(sink) = &self.sink {
                sink.pause();
            }
            self.state = PlayerState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == PlayerState::Paused {
            if let Some(sink) = &self.sink {
                sink.play();
            }
            self.state = PlayerState::Playing;
        }
    }

    pub fn stop(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.state = PlayerState::Stopped;
        self.current_track = None;
        self.audio_data = None;
        self.seek_offset_ms = 0;
    }

    pub fn toggle_playback(&mut self) {
        match self.state {
            PlayerState::Playing => self.pause(),
            PlayerState::Paused => self.resume(),
            PlayerState::Stopped => {}
        }
    }

    pub fn set_volume(&mut self, vol: f32) {
        // Quantise to nearest 5 % to prevent floating-point drift
        self.volume = (vol * 20.0).round() / 20.0;
        self.volume = self.volume.clamp(0.0, 2.0);
        if let Some(sink) = &self.sink {
            sink.set_volume(self.volume);
        }
    }

    pub fn adjust_volume(&mut self, delta: f32) {
        self.set_volume(self.volume + delta);
    }

    /// Cycle to the next play mode.
    pub fn cycle_play_mode(&mut self) {
        self.play_mode = self.play_mode.next_variant();
        // Reset shuffle state if entering shuffle mode
        self.shuffle_state = None;
    }

    /// Prepare shuffle queue for the given list length.
    #[allow(dead_code)]
    pub fn init_shuffle(&mut self, list_len: usize) {
        if self.play_mode == PlayMode::Shuffle {
            self.shuffle_state = Some(ShuffleState::new(list_len.max(1)));
        }
    }

    /// Pop the next shuffle index; caller should re-shuffle if `None`.
    pub fn next_shuffle_index(&mut self) -> Option<usize> {
        self.shuffle_state.as_mut().and_then(|s| s.next())
    }

    // ── Queries ──

    /// Check if playback has finished.
    pub fn is_finished(&self) -> bool {
        self.sink.as_ref().map_or(true, |s| s.empty())
    }

    pub fn is_stopped(&self) -> bool {
        self.state == PlayerState::Stopped
    }

    /// Current playback position in milliseconds (absolute, accounting for seek).
    pub fn position_ms(&self) -> u64 {
        let sink_pos = self
            .sink
            .as_ref()
            .map(|s| s.get_pos().as_millis() as u64)
            .unwrap_or(0);
        self.seek_offset_ms + sink_pos
    }

    pub fn state(&self) -> &PlayerState {
        &self.state
    }

    pub fn current_track(&self) -> Option<&TrackInfo> {
        self.current_track.as_ref()
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn has_audio_data(&self) -> bool {
        self.audio_data.is_some()
    }
}
