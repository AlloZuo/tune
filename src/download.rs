// ── Download primitives: streaming, cache ──

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::message::MainMessage;
use crate::player::{SharedAudioBuf, TrackInfo};
use crate::server::HTTP;

// ── Audio disk cache ──

/// On-disk cache for downloaded audio files with automatic eviction.
///
/// - **Max size**: 2 GB — oldest files (by mtime) are deleted when exceeded.
/// - **Max age**: 30 days — expired files are purged on cache init.
/// - **Key**: hash of `(server_id, absolute_path)` → safe short filename.
///
/// Location: `<system-temp>/tune-cache/`
const CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const CACHE_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;

pub struct AudioCache {
    dir: PathBuf,
}

use std::hash::{Hash, Hasher};

impl AudioCache {
    /// Create the cache directory.
    /// Call `init_async()` before the first `put()` to purge expired entries.
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join("tune-cache");
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }

    /// Async init: purge expired cache files (async I/O).
    pub async fn init_async(&self) {
        self.purge_expired().await;
    }

    /// Filesystem-safe cache key for a song.
    pub fn path_for(&self, server_id: &str, path: &str) -> PathBuf {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        server_id.hash(&mut hasher);
        path.hash(&mut hasher);
        self.dir.join(format!("{:016x}", hasher.finish()))
    }

    /// Check whether a song is cached.
    pub fn has(&self, server_id: &str, path: &str) -> bool {
        self.path_for(server_id, path).exists()
    }

    /// Store audio data into the cache, then evict if over the size limit.
    pub async fn put(&self, server_id: &str, path: &str, data: &[u8]) {
        let p = self.path_for(server_id, path);
        let _ = tokio::fs::write(&p, data).await;
        self.evict_if_needed().await;
    }

    async fn purge_expired(&self) {
        let now = std::time::SystemTime::now();
        let mut entries = match tokio::fs::read_dir(&self.dir).await {
            Ok(r) => r,
            Err(_) => return,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                if let Ok(modified) = meta.modified() {
                    if now.duration_since(modified).map_or(false, |age| {
                        age.as_secs() > CACHE_MAX_AGE_SECS
                    }) {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    }

    async fn evict_if_needed(&self) {
        let mut entries = match tokio::fs::read_dir(&self.dir).await {
            Ok(r) => r,
            Err(_) => return,
        };
        let mut file_infos: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                file_infos.push((
                    entry.path(),
                    meta.len(),
                    meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                ));
            }
        }

        let total: u64 = file_infos.iter().map(|(_, size, _)| size).sum();
        if total <= CACHE_MAX_BYTES {
            return;
        }

        file_infos.sort_by_key(|(_, _, modified)| *modified);

        let mut excess = total.saturating_sub(CACHE_MAX_BYTES);
        for (path, size, _) in &file_infos {
            if excess == 0 {
                break;
            }
            if tokio::fs::remove_file(path).await.is_ok() {
                excess = excess.saturating_sub(*size);
            }
        }
    }
}

// ── Progressive streaming ──

/// Stream an HTTP response body into a `SharedAudioBuf` for progressive
/// playback.  Once enough initial data has been buffered, sends a
/// `StreamReady` message so the main loop can start playing immediately.
/// Continues downloading the remainder into the same buffer, reporting
/// progress along the way.
pub async fn download_http_stream(
    url: &str,
    tx: &mpsc::Sender<MainMessage>,
    buf: Arc<SharedAudioBuf>,
    track: TrackInfo,
) -> Result<Vec<u8>> {
    let response = HTTP.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {} from {}", status.as_u16(), url);
    }

    const STREAM_THRESHOLD: u64 = 256 * 1024; // 256 KB
    let total = response.content_length().unwrap_or(0);
    let mut received: u64 = 0;
    let mut data = Vec::with_capacity(total as usize);
    let mut stream = response.bytes_stream();
    let mut stream_ready_sent = false;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        received += chunk.len() as u64;
        data.extend_from_slice(&chunk);

        buf.push(&chunk);

        let _ = tx.send(MainMessage::DownloadProgress(received, total)).await;

        if !stream_ready_sent && received >= STREAM_THRESHOLD {
            stream_ready_sent = true;
            let _ = tx
                .send(MainMessage::StreamReady(buf.clone(), track.clone()))
                .await;
        }
    }

    // Tiny file (< 256 KB): send StreamReady after all data is in the buffer.
    if !stream_ready_sent {
        let _ = tx
            .send(MainMessage::StreamReady(buf.clone(), track))
            .await;
    }

    buf.set_eof();
    Ok(data)
}
