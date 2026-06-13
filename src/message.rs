// ── Inter-task message protocol ──
//
// Messages sent from background tasks (download, fetch) to the main event loop.

use std::sync::Arc;

use rodio::Source;

use crate::server::MusicEntry;
use crate::lyrics::Lyrics;
use crate::player::{SharedAudioBuf, TrackInfo};

pub enum MainMessage {
    MusicListLoaded(Vec<MusicEntry>),
    MusicListLoadFailed(String),
    AudioDownloaded(MusicEntry, Vec<u8>, Option<Lyrics>),
    AudioDownloadFailed(String),
    DownloadProgress(u64, u64), // (bytes_received, total_bytes)
    StreamReady(Arc<SharedAudioBuf>, TrackInfo),
    LyricsReady(Option<Lyrics>),
    /// A background seek task has finished decoding.
    /// (`source`, `pos_ms`, `is_forward` — for status message direction).
    SeekPrepared(Box<dyn Source<Item = f32> + Send>, u64, bool),
    /// Background seek task failed.
    SeekFailed(String),
}
