//! Local file system music server adapter.
//!
//! Reads music files from a local directory path (no network required).
//! Useful for playing music stored on the same machine.
//!
//! Supported audio formats: mp3, flac, ogg, wav, m4a, wma, aac, opus, aiff.

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use crate::server::{MusicEntry, MusicServer, ServerFeatures};

/// Try to read the audio duration (ms) from a file's metadata headers.
/// Returns 0 on any error (silent fallback).
fn probe_duration(path: &str) -> u64 {
    use lofty::file::AudioFile;
    use lofty::probe::Probe;

    match Probe::open(path) {
        Ok(probe) => match probe.read() {
            Ok(file) => file.properties().duration().as_millis() as u64,
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

/// Audio file extensions we recognise (case-insensitive).
const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "ogg", "wav", "m4a", "wma", "aac", "opus", "aiff", "wv", "ape",
];

pub struct LocalServer {
    /// Normalised root directory path.
    root: String,
}

impl LocalServer {
    pub fn new(root: &str) -> Self {
        Self {
            root: root.trim_end_matches('/').trim_end_matches('\\').to_string(),
        }
    }
}

#[async_trait]
impl MusicServer for LocalServer {
    fn name(&self) -> &str {
        "本地文件夹"
    }

    fn base_url(&self) -> &str {
        &self.root
    }

    fn features(&self) -> ServerFeatures {
        ServerFeatures {
            search: false,
            cover_art: false,
        }
    }
    async fn fetch_list(&self) -> Result<Vec<MusicEntry>> {
        let root = Path::new(&self.root).to_path_buf();
        let root_str = self.root.clone();
        if !Path::new(&root_str).exists() {
            anyhow::bail!("路径不存在: {}", self.root);
        }
        if !Path::new(&root_str).is_dir() {
            anyhow::bail!("不是文件夹: {}", self.root);
        }

        // Run blocking directory walk on a dedicated thread to avoid
        // stalling the tokio worker pool on large libraries.
        let entries = tokio::task::spawn_blocking(move || -> Result<Vec<MusicEntry>> {
            let walk_dir = |path: &Path| -> std::io::Result<Vec<std::path::PathBuf>> {
                let mut files = Vec::new();
                let mut dirs = vec![path.to_path_buf()];
                while let Some(dir) = dirs.pop() {
                    if let Ok(read) = std::fs::read_dir(&dir) {
                        for entry in read.flatten() {
                            let p = entry.path();
                            if p.is_dir() {
                                dirs.push(p);
                            } else if p.is_file() {
                                files.push(p);
                            }
                        }
                    }
                }
                Ok(files)
            };

            let mut entries = Vec::new();
            let files = walk_dir(&root)?;

            for file_path in files {
                if let Some(ext) = file_path.extension().and_then(|e| e.to_str())
                    && AUDIO_EXTS.contains(&ext.to_lowercase().as_str())
                {
                    let absolute_path = file_path.to_string_lossy().to_string();
                    let file_stem = file_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let artist = file_path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();

                    let duration = probe_duration(&absolute_path);

                    entries.push(MusicEntry {
                        absolute_path,
                        name: file_stem,
                        artist,
                        album: String::new(),
                        duration,
                        server_id: String::new(),
                    });
                }
            }

            Ok(entries)
        })
        .await
        .map_err(|e| anyhow::anyhow!("walk_dir thread panicked: {}", e))?
        .map_err(|e| anyhow::anyhow!("walk_dir failed: {}", e))?;

        Ok(entries)
    }


    fn stream_url(&self, music: &MusicEntry) -> String {
        // Return the local file path directly.
        // The caller uses fetch_audio() for local servers,
        // so this is only a fallback / informational value.
        music.absolute_path.clone()
    }

    async fn fetch_cover_data(&self, music: &MusicEntry) -> Option<Vec<u8>> {
        use lofty::file::TaggedFileExt;
        use lofty::probe::Probe;

        let file = Probe::open(&music.absolute_path).ok()?.read().ok()?;
        let tag = file.primary_tag().or_else(|| file.first_tag())?;
        let picture = tag.pictures().first()?;
        let data = picture.data();
        if data.is_empty() { None } else { Some(data.to_vec()) }
    }

    async fn fetch_audio(&self, music: &MusicEntry) -> Result<Vec<u8>> {
        let data = tokio::fs::read(&music.absolute_path).await?;
        Ok(data)
    }
}
