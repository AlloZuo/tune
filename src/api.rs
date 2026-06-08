use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── Shared data types ──

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MusicEntry {
    #[allow(dead_code)]
    pub id: u64,
    #[serde(rename = "absoultePath")]
    pub absolute_path: String,
    pub name: String,
    pub artist: String,
    /// Duration in milliseconds
    pub duration: u64,
    #[allow(dead_code)]
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct MusicListResponse {
    pub children: Vec<MusicEntry>,
}

// ── MusicServer trait ──

/// Abstract interface for a music server backend.
///
/// Each server type implements this trait to provide:
/// - A list of available music entries
/// - A streamable URL for any given file path
#[async_trait]
pub trait MusicServer: Send + Sync {
    /// Human-readable server type name (e.g. "文件闪传", "Navidrome").
    fn name(&self) -> &str;

    /// Base URL of the server.
    fn base_url(&self) -> &str;

    /// Fetch the complete list of music entries from the server.
    async fn fetch_list(&self) -> Result<Vec<MusicEntry>>;

    /// Build a playable / streamable URL for the given absolute file path.
    fn stream_url(&self, absolute_path: &str) -> String;
}

// ── 文件闪传 (Fast File Transfer) adapter ──

/// Adapter for the 文件闪传 mobile app server.
///
/// Endpoints:
/// - `GET {base}/musicsV2` → `{ children: [...] }`
/// - `GET {base}/file?path={encoded_path}` → audio stream
pub struct FileTransferServer {
    base_url: String,
}

impl FileTransferServer {
    pub fn new(base_url: String) -> Self {
        Self { base_url }
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
        let resp: MusicListResponse = reqwest::get(&url).await?.json().await?;
        Ok(resp.children)
    }

    fn stream_url(&self, absolute_path: &str) -> String {
        let encoded_path = encode_url_path(absolute_path);
        format!("{}/file?path={}", self.base_url, encoded_path)
    }
}

fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            let mut encoded = String::new();
            for byte in segment.bytes() {
                match byte {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        encoded.push(byte as char);
                    }
                    b' ' => encoded.push_str("%20"),
                    _ => {
                        encoded.push_str(&format!("%{:02X}", byte));
                    }
                }
            }
            encoded
        })
        .collect::<Vec<_>>()
        .join("/")
}

// ── Factory ──

/// Create a music server adapter from a type identifier and base URL.
///
/// Supported server types:
/// - `"file-transfer"` — 文件闪传 (default)
///
/// Unknown types fall back to `file-transfer`.
pub fn create_server(server_type: &str, base_url: &str) -> Arc<dyn MusicServer> {
    match server_type {
        "file-transfer" => Arc::new(FileTransferServer::new(base_url.to_string())),
        _ => Arc::new(FileTransferServer::new(base_url.to_string())),
    }
}
