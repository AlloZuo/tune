use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use crate::lyrics::Lyrics;

pub mod file_transfer;
pub mod local;
pub mod navidrome;

// ── Password obfuscation ──

mod password_serde {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(pwd: &str, s: S) -> Result<S::Ok, S::Error> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(pwd);
        s.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        let s = String::deserialize(d)?;
        // Try base64 decode; fall back to plaintext for backward compat
        Ok(base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or(s))
    }
}

// ── Common URL encoding ──

/// Percent-encode a URL path component, preserving unreserved characters.
/// Used by both file_transfer and navidrome adapters.
pub(crate) fn encode_url_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// Shared HTTP client with a 15-second timeout per request.
pub(crate) static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to build reqwest Client")
});

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
    /// Album name (populated by servers that support grouping).
    #[serde(default)]
    pub album: String,
    #[allow(dead_code)]
    pub size: u64,
    /// Which server this entry came from (set by ServerPool).
    #[serde(default)]
    pub server_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MusicListResponse {
    pub children: Vec<MusicEntry>,
}

// ── Server configuration ──

/// Configuration passed to create_server() and stored on disk.
/// Extensible — add fields without breaking backward compatibility
/// (use #[serde(default)] for new fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Unique identifier (also used as display label).
    #[serde(default)]
    pub name: String,
    pub server_url: String,
    /// Server type identifier (e.g. "file-transfer", "navidrome").
    pub server_type: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    #[serde(with = "password_serde")]
    pub password: String,
    /// Whether this server is temporarily disabled (won't be polled for
    /// music lists / search, but existing songs still play).
    #[serde(default)]
    pub disabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            server_url: String::new(),
            server_type: "file-transfer".to_string(),
            username: String::new(),
            password: String::new(),
            disabled: false,
        }
    }
}

// ── Server features ──

/// What optional capabilities a server supports.
/// UI can check `features()` at runtime to decide which features to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub struct ServerFeatures {
    pub search: bool,
    pub cover_art: bool,
}

// ── MusicServer trait ──

/// Abstract interface for a music server backend.
///
/// Each server type implements this trait to provide:
/// - A list of available music entries
/// - A streamable URL for any given file path
/// - Optional: search, cover art
///
/// Note: `name`, `base_url`, `features`, `search`, and `cover_url` trigger
/// `dead_code` warnings because they are only used via `Arc<dyn MusicServer>`
/// dispatch, never called directly.
#[async_trait]
#[allow(dead_code)]
pub trait MusicServer: Send + Sync {
    /// Human-readable server type name (e.g. "文件闪传", "Navidrome").
    fn name(&self) -> &str;

    /// Base URL of the server.
    fn base_url(&self) -> &str;

    /// Advertise what optional features this server supports.
    fn features(&self) -> ServerFeatures {
        ServerFeatures::default()
    }

    /// Fetch the complete list of music entries from the server.
    async fn fetch_list(&self) -> Result<Vec<MusicEntry>>;

    /// Build a playable / streamable URL for the given music entry.
    fn stream_url(&self, music: &MusicEntry) -> String;

    /// Server-side search (optional).
    /// Default returns an error; override if the server supports it.
    async fn search(&self, _query: &str) -> Result<Vec<MusicEntry>> {
        Err(anyhow::anyhow!("this server does not support search"))
    }

    /// Cover art URL (optional).
    fn cover_url(&self, _music: &MusicEntry) -> Option<String> {
        None
    }

    /// Fetch lyrics from the server (optional).
    /// Default returns `None`; override if the server provides a lyrics API.
    async fn fetch_lyrics(&self, _music: &MusicEntry) -> Option<Lyrics> {
        None
    }

    /// Fetch the raw audio bytes for a music entry.
    ///
    /// Default implementation downloads from `stream_url()` via HTTP.
    /// Override for local files or custom protocols.
    async fn fetch_audio(&self, music: &MusicEntry) -> Result<Vec<u8>> {
        let url = self.stream_url(music);
        let response = HTTP.get(&url).send().await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("HTTP {} from {}", status.as_u16(), url);
        }
        Ok(response.bytes().await?.to_vec())
    }
}

// ── Factory ──

/// Create a single server adapter from a `ServerConfig`.
fn create_server(config: &ServerConfig) -> Arc<dyn MusicServer> {
    match config.server_type.as_str() {
        "file-transfer" => Arc::new(crate::server::file_transfer::FileTransferServer::new(&config.server_url)),
        "navidrome" | "subsonic" => {
            Arc::new(crate::server::navidrome::SubsonicServer::new(
                &config.server_url,
                &config.username,
                &config.password,
            ))
        }
        "local" => Arc::new(crate::server::local::LocalServer::new(&config.server_url)),
        _ => Arc::new(crate::server::file_transfer::FileTransferServer::new(&config.server_url)),
    }
}

/// Create a multi-server pool from a list of configs.
/// Returns a single `FileTransferServer` if the list is empty (fallback).
pub fn create_server_pool(configs: &[ServerConfig]) -> Arc<dyn MusicServer> {
    if configs.is_empty() {
        return Arc::new(crate::server::file_transfer::FileTransferServer::new(""));
    }
    let servers: Vec<(String, bool, Arc<dyn MusicServer>)> = configs
        .iter()
        .map(|cfg| {
            let id = if cfg.name.is_empty() {
                cfg.server_type.clone()
            } else {
                cfg.name.clone()
            };
            (id, cfg.disabled, create_server(cfg))
        })
        .collect();
    Arc::new(ServerPool { servers })
}

// ── ServerPool (multi-server dispatcher) ──

/// A `MusicServer` that aggregates multiple server backends.
/// Routes requests to the correct backend based on `MusicEntry.server_id`.
/// Each server entry is `(id, disabled, server)`.
pub struct ServerPool {
    servers: Vec<(String, bool, Arc<dyn MusicServer>)>,
}

impl ServerPool {
    fn find(&self, server_id: &str) -> Option<&Arc<dyn MusicServer>> {
        self.servers
            .iter()
            .find(|(id, _, _)| id == server_id)
            .map(|(_, _, s)| s)
    }
}

#[async_trait]
impl MusicServer for ServerPool {
    fn name(&self) -> &str {
        "ServerPool"
    }

    fn base_url(&self) -> &str {
        self.servers
            .first()
            .map(|(_, _, s)| s.base_url())
            .unwrap_or("")
    }

    async fn fetch_list(&self) -> Result<Vec<MusicEntry>> {
        let mut all = Vec::new();
        for (id, disabled, server) in &self.servers {
            if *disabled {
                continue;
            }
            match server.fetch_list().await {
                Ok(mut entries) => {
                    for entry in &mut entries {
                        entry.server_id = id.clone();
                    }
                    all.extend(entries);
                }
                Err(e) => {
                    crate::log_error!("服务器「{}」获取列表失败: {}", id, e);
                }
            }
        }
        Ok(all)
    }

    fn stream_url(&self, music: &MusicEntry) -> String {
        if let Some(server) = self.find(&music.server_id) {
            server.stream_url(music)
        } else {
            // Fallback: first non-disabled server
            self.servers
                .iter()
                .find(|(_, disabled, _)| !disabled)
                .map(|(_, _, s)| s.stream_url(music))
                .or_else(|| self.servers.first().map(|(_, _, s)| s.stream_url(music)))
                .unwrap_or_default()
        }
    }

    async fn search(&self, query: &str) -> Result<Vec<MusicEntry>> {
        let mut all = Vec::new();
        for (id, disabled, server) in &self.servers {
            if *disabled {
                continue;
            }
            if let Ok(mut entries) = server.search(query).await {
                for entry in &mut entries {
                    entry.server_id = id.clone();
                }
                all.extend(entries);
            }
        }
        Ok(all)
    }

    fn cover_url(&self, music: &MusicEntry) -> Option<String> {
        self.find(&music.server_id)
            .and_then(|s| s.cover_url(music))
    }

    async fn fetch_lyrics(&self, music: &MusicEntry) -> Option<Lyrics> {
        match self.find(&music.server_id) {
            Some(server) => server.fetch_lyrics(music).await,
            None => None,
        }
    }

    async fn fetch_audio(&self, music: &MusicEntry) -> Result<Vec<u8>> {
        match self.find(&music.server_id) {
            Some(server) => server.fetch_audio(music).await,
            None => {
                let url = self.stream_url(music);
                let response = HTTP.get(&url).send().await?;
                let status = response.status();
                if !status.is_success() {
                    anyhow::bail!("HTTP {} from {}", status.as_u16(), url);
                }
                Ok(response.bytes().await?.to_vec())
            }
        }
    }
}
