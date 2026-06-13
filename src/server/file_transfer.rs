//! 文件闪传 (Fast File Transfer) adapter.
//!
//! Endpoints:
//! - `GET {base}/musicsV2` → `{ children: [...] }`
//! - `GET {base}/file?path={encoded_path}` → audio stream

use std::io::{BufReader, Cursor};

use anyhow::Result;
use async_trait::async_trait;

use crate::server::{encode_url_component, MusicEntry, MusicListResponse, MusicServer, HTTP};

/// Adapter for the 文件闪传 mobile app server.
pub struct FileTransferServer {
    base_url: String,
}

impl FileTransferServer {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }
}

#[async_trait]
impl MusicServer for FileTransferServer {
    fn name(&self) -> &str {
        "文件闪传"
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn fetch_list(&self) -> Result<Vec<MusicEntry>> {
        let url = format!("{}/musicsV2", self.base_url);
        let resp: MusicListResponse = HTTP.get(&url).send().await?.json().await?;
        Ok(resp.children)
    }

    fn stream_url(&self, music: &MusicEntry) -> String {
        let encoded_path = encode_url_path(&music.absolute_path);
        format!("{}/file?path={}", self.base_url, encoded_path)
    }

    async fn fetch_cover_data(&self, music: &MusicEntry) -> Option<Vec<u8>> {
        // Download the audio file first, then extract the embedded cover via lofty.
        // We infer the FileType from the file extension so lofty can correctly
        // identify the format even when reading from an in-memory buffer.
        use lofty::file::TaggedFileExt;
        use lofty::probe::Probe;

        let url = self.stream_url(music);
        let response = HTTP.get(&url).send().await.ok()?;
        let audio_bytes = response.bytes().await.ok()?;
        if audio_bytes.is_empty() {
            return None;
        }
        let cursor = Cursor::new(audio_bytes.to_vec());
        let file_type = file_type_from_ext(&music.absolute_path);
        let reader = BufReader::new(cursor);
        let probe = match file_type {
            Some(ft) => Probe::with_file_type(reader, ft),
            None => Probe::new(reader),
        };
        let file = probe.read().ok()?;
        let tag = file.primary_tag().or_else(|| file.first_tag())?;
        let picture = tag.pictures().first()?;
        let data = picture.data();
        if data.is_empty() { None } else { Some(data.to_vec()) }
    }
}

fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(encode_url_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// Map a file extension to a lofty [`FileType`], so that in-memory parsing
/// has a format hint instead of relying solely on magic-byte sniffing.
fn file_type_from_ext(path: &str) -> Option<lofty::file::FileType> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    match ext.as_str() {
        "mp3" => Some(lofty::file::FileType::Mpeg),
        "flac" => Some(lofty::file::FileType::Flac),
        "ogg" | "opus" => Some(lofty::file::FileType::Vorbis),
        "wav" => Some(lofty::file::FileType::Wav),
        "m4a" | "m4b" | "mp4" => Some(lofty::file::FileType::Mp4),
        "aiff" | "aif" => Some(lofty::file::FileType::Aiff),
        "wv" => Some(lofty::file::FileType::WavPack),
        "ape" => Some(lofty::file::FileType::Ape),
        _ => None,
    }
}
