use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::Result;
use rand::rng;
use rand::Rng;
use rand::seq::SliceRandom;
use rodio::{
    Decoder, MixerDeviceSink, Source,
};
use rodio::Player as RodioPlayer;

use crate::server::MusicEntry;

/// Result type for the blocking decode step: a decoded source plus optional total duration.
type DecodeResult = Result<(rodio::Decoder<std::io::Cursor<Vec<u8>>>, Option<std::time::Duration>)>;

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

    pub fn label(self) -> String {
        match self {
            PlayMode::Sequential => crate::tf!("playmode.sequential"),
            PlayMode::SingleRepeat => crate::tf!("playmode.single_repeat"),
            PlayMode::Shuffle => crate::tf!("playmode.shuffle"),
        }
    }

    pub fn short_label(self) -> String {
        match self {
            PlayMode::Sequential => crate::tf!("playmode.short_sequential"),
            PlayMode::SingleRepeat => crate::tf!("playmode.short_single"),
            PlayMode::Shuffle => crate::tf!("playmode.short_shuffle"),
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
    ///
    /// If `exclude` is `Some`, the last element (first to be popped) is
    /// guaranteed not to equal `exclude`, preventing immediate repeats
    /// when reshuffling after the queue was exhausted.
    pub fn new(count: usize, exclude: Option<usize>) -> Self {
        let mut indices: Vec<usize> = (0..count).collect();
        indices.shuffle(&mut rng());
        if let Some(ex) = exclude
            && count > 1 && indices.last() == Some(&ex) {
                // Swap the last element (which will be popped first) with
                // a random element elsewhere, ensuring no immediate repeat.
                let swap = rand::rng().random_range(0..count - 1);
                let len = indices.len();
                indices.swap(len - 1, swap);
            }
        Self { remaining: indices }
    }

    /// Pop the next index; returns `None` if the queue is empty.
    pub fn next(&mut self) -> Option<usize> {
        self.remaining.pop()
    }

}

// ── Play queue ──

/// A FIFO queue of songs to play before the normal playback order resumes.
/// When non-empty, auto-next consumes from this queue first.
/// Once empty, the player falls back to sequential / shuffle behaviour.
#[derive(Debug, Clone)]
pub struct PlayQueue {
    songs: Vec<MusicEntry>,
}

impl PlayQueue {
    pub fn new() -> Self {
        Self { songs: Vec::new() }
    }

    /// Append a song to the end of the queue.
    pub fn push_back(&mut self, song: MusicEntry) {
        self.songs.push(song);
    }

    /// Insert a song at the front of the queue ("play next").
    pub fn push_front(&mut self, song: MusicEntry) {
        self.songs.insert(0, song);
    }

    /// Remove and return the first item.
    pub fn pop_front(&mut self) -> Option<MusicEntry> {
        if self.songs.is_empty() {
            None
        } else {
            Some(self.songs.remove(0))
        }
    }

    /// Remove the item at `index`.
    pub fn remove(&mut self, index: usize) -> Option<MusicEntry> {
        if index < self.songs.len() {
            Some(self.songs.remove(index))
        } else {
            None
        }
    }

    /// Move the item at `index` one position earlier (towards the front).
    pub fn move_up(&mut self, index: usize) {
        if index > 0 && index < self.songs.len() {
            self.songs.swap(index, index - 1);
        }
    }

    /// Move the item at `index` one position later (towards the back).
    pub fn move_down(&mut self, index: usize) {
        if index + 1 < self.songs.len() {
            self.songs.swap(index, index + 1);
        }
    }

    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&MusicEntry> {
        self.songs.get(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &MusicEntry> {
        self.songs.iter()
    }

}

// ── Progressive streaming buffer ──

/// A shared, growable audio buffer for progressive streaming.
///
/// The download task calls `push()` as chunks arrive, then `set_eof()`
/// when finished. The decoder reads via `StreamingCursor`, blocking
/// when no data is available yet (safe because rodio runs on its own
/// thread pool, not the tokio event loop).
pub struct SharedAudioBuf {
    inner: Mutex<SharedAudioBufInner>,
    data_ready: Condvar,
}

struct SharedAudioBufInner {
    data: Vec<u8>,
    eof: bool,
}

impl SharedAudioBuf {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(SharedAudioBufInner {
                data: Vec::new(),
                eof: false,
            }),
            data_ready: Condvar::new(),
        })
    }

    /// Append a chunk of data and wake any blocked reader.
    pub fn push(&self, chunk: &[u8]) {
        let mut inner = self.inner.lock().expect("SharedAudioBuf lock poisoned");
        inner.data.extend_from_slice(chunk);
        self.data_ready.notify_all();
    }

    /// Mark the stream as complete. Any blocked reader will see EOF.
    pub fn set_eof(&self) {
        let mut inner = self.inner.lock().expect("SharedAudioBuf lock poisoned");
        inner.eof = true;
        self.data_ready.notify_all();
    }

}

/// A `Read + Seek` wrapper around `SharedAudioBuf` for rodio's decoder.
///
/// `read()` blocks via condvar when no data is available, waiting for the
/// download task to push more bytes. This is safe because rodio drives the
/// decoder on its own background thread, not the tokio event loop.
pub struct StreamingCursor {
    buf: Arc<SharedAudioBuf>,
    pos: u64,
}

impl StreamingCursor {
    pub fn new(buf: Arc<SharedAudioBuf>) -> Self {
        Self { buf, pos: 0 }
    }
}

/// Timeout for the streaming condvar — if no data arrives within this window
/// the reader signals EOF instead of blocking forever (prevents hang on stall).
const STREAM_TIMEOUT: Duration = Duration::from_secs(5);

impl Read for StreamingCursor {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut guard = self.buf.inner.lock().expect("SharedAudioBuf lock poisoned");
        loop {
            if (self.pos as usize) < guard.data.len() {
                let available = guard.data.len() - self.pos as usize;
                let to_read = buf.len().min(available);
                let start = self.pos as usize;
                buf[..to_read].copy_from_slice(&guard.data[start..start + to_read]);
                self.pos += to_read as u64;
                return Ok(to_read);
            }
            if guard.eof {
                return Ok(0);
            }
            // Block until more data arrives, EOF is set, or timeout elapses.
            let (new_guard, wait_result) = self
                .buf
                .data_ready
                .wait_timeout(guard, STREAM_TIMEOUT)
                .expect("SharedAudioBuf condvar wait poisoned");
            guard = new_guard;
            if wait_result.timed_out() {
                // Stream appears stalled — signal EOF so the decoder stops.
                return Ok(0);
            }
        }
    }
}

impl Seek for StreamingCursor {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let guard = self.buf.inner.lock().expect("SharedAudioBuf lock poisoned");
        let new_pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(offset) => {
                let new = self.pos as i64 + offset;
                if new >= 0 {
                    new as u64
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "seek before start",
                    ));
                }
            }
            SeekFrom::End(offset) => {
                let end = guard.data.len() as i64;
                let new = end + offset;
                if new >= 0 {
                    new as u64
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "seek before start",
                    ));
                }
            }
        };
        self.pos = new_pos;
        Ok(new_pos)
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
    pub absolute_path: String,
    pub total_duration_ms: u64,
    pub lyrics: Option<Lyrics>,
}

// ── Player ──

pub struct Player {
    rodio_player: Option<RodioPlayer>,
    _device_sink: MixerDeviceSink,
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

    /// Songs queued to play next (checked before shuffle/sequential).
    pub queue: PlayQueue,

    /// When `Some`, progressive streaming is in progress and the player is
    /// already consuming audio from this buffer while the download continues.
    pub streaming_buf: Option<Arc<SharedAudioBuf>>,
}

impl Player {
    pub fn new() -> Result<Self> {
        let device_sink = rodio::DeviceSinkBuilder::open_default_sink()?;
        Ok(Self {
            rodio_player: None,
            _device_sink: device_sink,
            state: PlayerState::Stopped,
            current_track: None,
            volume: 1.0,
            audio_data: None,
            seek_offset_ms: 0,
            play_mode: PlayMode::Sequential,
            shuffle_state: None,
            queue: PlayQueue::new(),
            streaming_buf: None,
        })
    }

    /// Async version: decode audio in a blocking thread pool to avoid
    /// blocking the async runtime during CPU-intensive symphonia decode.
    pub async fn play_bytes_async(&mut self, data: Vec<u8>, track: TrackInfo) -> Result<()> {
        self.audio_data = Some(data.clone());

        let (source, decoded_duration) = {
            let result: DecodeResult =
                tokio::task::spawn_blocking(move || {
                    let cursor = Cursor::new(data);
                    let source = Decoder::builder()
                        .with_data(cursor)
                        .with_seekable(true)
                        .build()?;
                    let dur = source.total_duration();
                    Ok::<_, anyhow::Error>((source, dur))
                })
                .await
                .map_err(|e| anyhow::anyhow!("decode task panicked: {}", e))?;
            result?
        };

        if let Some(rodio_player) = &self.rodio_player {
            rodio_player.stop();
        }

        let mut track = track;
        if track.total_duration_ms == 0
            && let Some(dur) = decoded_duration {
                track.total_duration_ms = dur.as_millis() as u64;
            }

        let rodio_player = RodioPlayer::connect_new(self._device_sink.mixer());
        rodio_player.set_volume(self.volume);
        rodio_player.append(source);

        self.rodio_player = Some(rodio_player);
        self.seek_offset_ms = 0;
        self.state = PlayerState::Playing;
        self.current_track = Some(track);
        self.streaming_buf = None;
        Ok(())
    }

    /// Start playing from a progressive stream. The `SharedAudioBuf` is
    /// shared with a background download task that keeps pushing bytes.
    ///
    /// The decoder blocks on `read()` when no data is available, which is
    /// safe because rodio drives the decoder on its own thread.
    pub fn play_streaming(&mut self, buf: Arc<SharedAudioBuf>, track: TrackInfo) -> Result<()> {
        // StreamReady is only sent after 256 KB has been buffered,
        // so the data should already be available. Use a non-blocking
        // check instead of wait_while to avoid freezing the main loop.
        const MIN_BYTES: usize = 256 * 1024;
        {
            let guard = buf.inner.lock().expect("SharedAudioBuf lock poisoned");
            if guard.data.len() < MIN_BYTES && !guard.eof {
                anyhow::bail!("Not enough data buffered for streaming (have {} bytes, need {})", guard.data.len(), MIN_BYTES);
            }
            if guard.data.is_empty() {
                anyhow::bail!("No audio data received, download may have failed");
            }
        }

        let cursor = StreamingCursor::new(buf.clone());
        let source = Decoder::builder()
            .with_data(cursor)
            .with_seekable(true)
            .build()?;

        if let Some(rodio_player) = &self.rodio_player {
            rodio_player.stop();
        }

        let mut track = track;
        if track.total_duration_ms == 0
            && let Some(dur) = source.total_duration() {
                track.total_duration_ms = dur.as_millis() as u64;
            }

        let rodio_player = RodioPlayer::connect_new(self._device_sink.mixer());
        rodio_player.set_volume(self.volume);
        rodio_player.append(source);

        self.rodio_player = Some(rodio_player);
        self.seek_offset_ms = 0;
        self.state = PlayerState::Playing;
        self.current_track = Some(track);
        self.streaming_buf = Some(buf);
        // Don't overwrite `audio_data` — it will be set later via
        // `finalize_streaming()` once the download completes.
        Ok(())
    }

    /// Called when the progressive download has finished.
    /// Stores the full audio data (for seeking) and updates lyrics.
    /// Update lyrics on the current track after playback has started.
    /// Used when lyrics arrive asynchronously (LyricsReady message).
    pub fn set_lyrics(&mut self, lyrics: Option<Lyrics>) {
        if let Some(ref mut track) = self.current_track
            && lyrics.is_some() {
                track.lyrics = lyrics;
            }
    }

    pub fn finalize_streaming(&mut self, data: Vec<u8>, lyrics: Option<Lyrics>) {
        self.audio_data = Some(data);
        if let Some(ref mut track) = self.current_track
            && lyrics.is_some() {
                track.lyrics = lyrics;
            }
        self.streaming_buf = None;
    }

    // ── Seek ──
    /// Seek to an absolute position in milliseconds.
    pub fn seek_to_ms(&mut self, pos_ms: u64) -> Result<()> {
        let data = if let Some(ref buf) = self.streaming_buf {
            // If the download is still in progress, wait for it to complete
            // so we have the full file for re-decoding.
            let mut guard = buf.inner.lock().expect("SharedAudioBuf lock poisoned");
            if !guard.eof {
                // Block until EOF so we can re-decode from scratch.
                guard = buf
                    .data_ready
                    .wait_while(guard, |state| !state.eof)
                    .map_err(|_| anyhow::anyhow!("Timed out waiting for streaming download to complete"))?;
            }
            let full = guard.data.clone();
            drop(guard);
            self.audio_data = Some(full.clone());
            self.streaming_buf = None;
            full
        } else {
            self.audio_data
                .clone()
                .ok_or_else(|| anyhow::anyhow!("No audio data available for seeking"))?
        };

        let total = self
            .current_track
            .as_ref()
            .map(|t| t.total_duration_ms)
            .unwrap_or(0);
        let pos_ms = pos_ms.min(total.saturating_sub(200)); // leave 200ms margin
        let seek_dur = Duration::from_millis(pos_ms);

        // Use DecoderBuilder with byte_len so symphonia's FLAC demuxer
        // can do binary seeking (needs byte_len for the search range).
        let byte_len = data.len() as u64;
        let cursor = Cursor::new(data.clone());
        let mut source = Decoder::builder()
            .with_data(cursor)
            .with_byte_len(byte_len)
            .with_seekable(true)
            .build()?;

        let seeked_source: Box<dyn Source<Item = f32> + Send> = match source.try_seek(seek_dur) {
            Ok(()) => Box::new(source),
            Err(e) => {
                eprintln!("Seek failed ({e}), falling back to skip_duration");
                let cursor2 = Cursor::new(data.clone());
                let source2 = Decoder::builder()
                    .with_data(cursor2)
                    .with_seekable(true)
                    .build()?;
                Box::new(source2.skip_duration(seek_dur))
            }
        };

        if let Some(rodio_player) = &self.rodio_player {
            rodio_player.stop();
        }
        let rodio_player = RodioPlayer::connect_new(self._device_sink.mixer());
        rodio_player.set_volume(self.volume);
        rodio_player.append(seeked_source);

        self.rodio_player = Some(rodio_player);
        self.seek_offset_ms = pos_ms;
        self.state = PlayerState::Playing;
        Ok(())
    }

    /// Async seek: decode audio in a blocking thread pool to avoid
    /// blocking the async runtime during re-decoding for seek.
    pub async fn seek_to_ms_async(&mut self, pos_ms: u64) -> Result<()> {
        let data = if let Some(ref buf) = self.streaming_buf {
            let mut guard = buf.inner.lock().expect("SharedAudioBuf lock poisoned");
            if !guard.eof {
                guard = buf
                    .data_ready
                    .wait_while(guard, |state| !state.eof)
                    .map_err(|_| anyhow::anyhow!("Timed out waiting for streaming download to complete"))?;
            }
            let full = guard.data.clone();
            drop(guard);
            self.audio_data = Some(full.clone());
            self.streaming_buf = None;
            full
        } else {
            self.audio_data
                .clone()
                .ok_or_else(|| anyhow::anyhow!("No audio data available for seeking"))?
        };

        let total = self
            .current_track
            .as_ref()
            .map(|t| t.total_duration_ms)
            .unwrap_or(0);
        let pos_ms = pos_ms.min(total.saturating_sub(200));
        let seek_dur = Duration::from_millis(pos_ms);

        let data_for_fallback = data.clone();
        let byte_len = data.len() as u64;
        let seeked_source: Box<dyn Source<Item = f32> + Send> =
            {
                let result: Result<Box<dyn Source<Item = f32> + Send>> =
                    tokio::task::spawn_blocking(move || {
                        let cursor = Cursor::new(data);
                        let mut source = Decoder::builder()
                            .with_data(cursor)
                            .with_byte_len(byte_len)
                            .with_seekable(true)
                            .build()?;

                        let seeked: Box<dyn Source<Item = f32> + Send> =
                            match source.try_seek(seek_dur) {
                                Ok(()) => Box::new(source),
                                Err(e) => {
                                    eprintln!("Seek failed ({e}), falling back to skip_duration");
                                    let cursor2 = Cursor::new(data_for_fallback);
                                    let source2 = Decoder::builder()
                                        .with_data(cursor2)
                                        .with_seekable(true)
                                        .build()?;
                                    Box::new(source2.skip_duration(seek_dur))
                                }
                            };
                        Ok::<_, anyhow::Error>(seeked)
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("seek task panicked: {}", e))?;
                result?
            };

        if let Some(rodio_player) = &self.rodio_player {
            rodio_player.stop();
        }
        let rodio_player = RodioPlayer::connect_new(self._device_sink.mixer());
        rodio_player.set_volume(self.volume);
        rodio_player.append(seeked_source);

        self.rodio_player = Some(rodio_player);
        self.seek_offset_ms = pos_ms;
        self.state = PlayerState::Playing;
        Ok(())
    }

    /// Extract raw audio data for a non-blocking seek.
    /// Returns `(audio_data, clamped_pos_ms)` or `None` if no data available.
    /// After calling this, the stream buffer (if any) is consumed and stored
    /// into `audio_data`. The caller should spawn `decode_seek_source()` on a
    /// blocking thread and then call `apply_seek_source()` when done.
    pub fn extract_seek_data(&mut self, delta_secs: i64) -> Option<(Vec<u8>, u64)> {
        let cur = self.position_ms() as i64;
        let raw_pos = (cur + delta_secs * 1000).max(0) as u64;

        let data = if let Some(ref buf) = self.streaming_buf {
            let guard = buf.inner.lock().ok()?;
            if !guard.eof {
                return None; // streaming not done yet — can't seek
            }
            let full = guard.data.clone();
            drop(guard);
            self.audio_data = Some(full.clone());
            self.streaming_buf = None;
            full
        } else {
            self.audio_data.clone()?
        };

        let total = self
            .current_track
            .as_ref()
            .map(|t| t.total_duration_ms)
            .unwrap_or(0);
        let pos_ms = raw_pos.min(total.saturating_sub(200));

        Some((data, pos_ms))
    }

    /// Apply a pre-decoded seek source to the player's sink.
    /// Call this on the main loop when `SeekPrepared` is received.
    pub fn apply_seek_source(
        &mut self,
        source: Box<dyn Source<Item = f32> + Send>,
        pos_ms: u64,
    ) {
        if let Some(rodio_player) = &self.rodio_player {
            rodio_player.stop();
        }
        let rodio_player = RodioPlayer::connect_new(self._device_sink.mixer());
        rodio_player.set_volume(self.volume);
        rodio_player.append(source);
        self.rodio_player = Some(rodio_player);
        self.seek_offset_ms = pos_ms;
        self.state = PlayerState::Playing;
    }



    // ── Playback control ──

    pub fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            if let Some(rodio_player) = &self.rodio_player {
                rodio_player.pause();
            }
            self.state = PlayerState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == PlayerState::Paused {
            if let Some(rodio_player) = &self.rodio_player {
                rodio_player.play();
            }
            self.state = PlayerState::Playing;
        }
    }

    pub fn stop(&mut self) {
        if let Some(rodio_player) = &self.rodio_player {
            rodio_player.stop();
        }
        self.state = PlayerState::Stopped;
        self.current_track = None;
        self.audio_data = None;
        self.seek_offset_ms = 0;
        self.streaming_buf = None;
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
        if let Some(rodio_player) = &self.rodio_player {
            rodio_player.set_volume(self.volume);
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

    /// Pop the next shuffle index; caller should re-shuffle if `None`.
    pub fn next_shuffle_index(&mut self) -> Option<usize> {
        self.shuffle_state.as_mut().and_then(|s| s.next())
    }

    // ── Queries ──

    /// Check if playback has finished.
    pub fn is_finished(&self) -> bool {
        self.rodio_player.as_ref().is_none_or(|p| p.empty())
    }

    /// Current playback position in milliseconds (absolute, accounting for seek).
    pub fn position_ms(&self) -> u64 {
        let sink_pos = self
            .rodio_player
            .as_ref()
            .map(|p| p.get_pos().as_millis() as u64)
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

// ── Non-blocking seek: decode in blocking thread pool ──

/// Decode audio data and seek to the given position.
/// Runs CPU-intensive symphonia decode on a blocking thread.
/// Returns a decoded source ready for `Sink::append`.
pub async fn decode_seek_source(
    data: Vec<u8>,
    pos_ms: u64,
) -> Result<Box<dyn Source<Item = f32> + Send>> {
    let data_for_fallback = data.clone();
    let byte_len = data.len() as u64;
    let seek_dur = Duration::from_millis(pos_ms);

    let result: Result<Box<dyn Source<Item = f32> + Send>> =
        tokio::task::spawn_blocking(move || {
            let cursor = Cursor::new(data);
            let mut source = Decoder::builder()
                .with_data(cursor)
                .with_byte_len(byte_len)
                .with_seekable(true)
                .build()?;

            let seeked: Box<dyn Source<Item = f32> + Send> = match source.try_seek(seek_dur) {
                Ok(()) => Box::new(source),
                Err(e) => {
                    eprintln!("Seek failed ({e}), falling back to skip_duration");
                    let cursor2 = Cursor::new(data_for_fallback);
                    let source2 = Decoder::builder()
                        .with_data(cursor2)
                        .with_seekable(true)
                        .build()?;
                    Box::new(source2.skip_duration(seek_dur))
                }
            };
            Ok::<_, anyhow::Error>(seeked)
        })
        .await
        .map_err(|e| anyhow::anyhow!("seek task panicked: {}", e))?;

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::MusicEntry;

    fn make_song(name: &str) -> MusicEntry {
        MusicEntry {
            absolute_path: format!("/music/{}.mp3", name),
            name: name.to_string(),
            artist: "test".to_string(),
            album: String::new(),
            duration: 200_000,
            server_id: "test-server".to_string(),
        }
    }

    // ── PlayQueue ──

    #[test]
    fn test_queue_new_is_empty() {
        let q = PlayQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn test_push_back_and_pop_front() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        assert_eq!(q.len(), 2);

        let a = q.pop_front().unwrap();
        assert_eq!(a.name, "A");
        assert_eq!(q.len(), 1);

        let b = q.pop_front().unwrap();
        assert_eq!(b.name, "B");
        assert!(q.is_empty());
    }

    #[test]
    fn test_push_front_play_next() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_front(make_song("B"));
        // B should be first (play next)
        assert_eq!(q.pop_front().unwrap().name, "B");
        assert_eq!(q.pop_front().unwrap().name, "A");
    }

    #[test]
    fn test_pop_front_empty_returns_none() {
        let mut q = PlayQueue::new();
        assert!(q.pop_front().is_none());
    }

    #[test]
    fn test_remove_by_index() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.push_back(make_song("C"));

        let b = q.remove(1).unwrap();
        assert_eq!(b.name, "B");
        assert_eq!(q.len(), 2);

        // Remaining: A, C
        assert_eq!(q.get(0).unwrap().name, "A");
        assert_eq!(q.get(1).unwrap().name, "C");
    }

    #[test]
    fn test_remove_out_of_bounds_returns_none() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        assert!(q.remove(5).is_none());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn test_move_up() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.push_back(make_song("C"));

        q.move_up(2); // C → [A, C, B]
        assert_eq!(q.get(0).unwrap().name, "A");
        assert_eq!(q.get(1).unwrap().name, "C");
        assert_eq!(q.get(2).unwrap().name, "B");

        q.move_up(1); // C → [C, A, B]
        assert_eq!(q.get(0).unwrap().name, "C");
    }

    #[test]
    fn test_move_up_first_element_does_nothing() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.move_up(0); // A is already first
        assert_eq!(q.get(0).unwrap().name, "A");
        assert_eq!(q.get(1).unwrap().name, "B");
    }

    #[test]
    fn test_move_down() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.push_back(make_song("C"));

        q.move_down(0); // A → [B, A, C]
        assert_eq!(q.get(0).unwrap().name, "B");
        assert_eq!(q.get(1).unwrap().name, "A");
        assert_eq!(q.get(2).unwrap().name, "C");
    }

    #[test]
    fn test_move_down_last_element_does_nothing() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.move_down(1);
        assert_eq!(q.get(0).unwrap().name, "A");
        assert_eq!(q.get(1).unwrap().name, "B");
    }

    #[test]
    fn test_clear() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn test_get_and_iter() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));

        assert_eq!(q.get(0).unwrap().name, "A");
        assert_eq!(q.get(1).unwrap().name, "B");
        assert!(q.get(2).is_none());

        let names: Vec<&str> = q.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    #[test]
    fn test_fifo_order() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("X"));
        q.push_back(make_song("Y"));
        q.push_back(make_song("Z"));

        assert_eq!(q.pop_front().unwrap().name, "X");
        assert_eq!(q.pop_front().unwrap().name, "Y");
        assert_eq!(q.pop_front().unwrap().name, "Z");
        assert!(q.is_empty());
    }

    #[test]
    fn test_push_front_after_push_back() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.push_front(make_song("C")); // C jumps ahead of A

        assert_eq!(q.pop_front().unwrap().name, "C");
        assert_eq!(q.pop_front().unwrap().name, "A");
        assert_eq!(q.pop_front().unwrap().name, "B");
    }

    #[test]
    fn test_interleaved_add_remove() {
        let mut q = PlayQueue::new();
        q.push_back(make_song("A"));
        q.push_back(make_song("B"));
        q.pop_front(); // remove A
        q.push_back(make_song("C"));
        q.push_front(make_song("D")); // D jumps to front

        // Queue: D, B, C
        assert_eq!(q.pop_front().unwrap().name, "D");
        assert_eq!(q.pop_front().unwrap().name, "B");
        assert_eq!(q.pop_front().unwrap().name, "C");
        assert!(q.is_empty());
    }
}
