/// Navidrome (Subsonic API) adapter.
///
/// Subsonic API is a de-facto standard used by Navidrome, Airsonic,
/// Gonic, Ampache, and many other self-hosted music servers.
///
/// API Docs: http://www.subsonic.org/pages/api.jsp
///
/// Endpoints used:
/// - `GET /rest/getAlbumList2` — browse albums
/// - `GET /rest/getAlbum` — get songs in an album
/// - `GET /rest/stream` — stream audio by song ID
/// - `GET /rest/search3` — search songs
/// - `GET /rest/getCoverArt` — album art URL

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;

use crate::server::{HTTP, MusicEntry, MusicServer, ServerFeatures};
use crate::lyrics::Lyrics;

// ── Server struct ──

pub struct SubsonicServer {
    base_url: String,
    username: String,
    password: String,
}

impl SubsonicServer {
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.to_string(),
        }
    }

    /// Build the auth query string that every Subsonic request needs.
    fn auth_params(&self) -> String {
        // Subsonic uses query-param auth: u=xx&p=xx&v=1.16.0&c=tune&f=json
        format!(
            "u={}&p={}&v=1.16.0&c=tune&f=json",
            self.username, self.password
        )
    }

    /// Check HTTP status and return a useful error message on failure.
    fn check_status(status: reqwest::StatusCode, endpoint: &str) -> Result<()> {
        if status.is_success() {
            Ok(())
        } else {
            let body = status.canonical_reason().unwrap_or("unknown");
            Err(anyhow::anyhow!(
                "{} 返回 HTTP {} {}",
                endpoint,
                status.as_u16(),
                body
            ))
        }
    }

    /// Fetch songs belonging to an album.
    async fn fetch_album_songs(&self, album_id: &str) -> Result<Vec<MusicEntry>> {
        let url = format!(
            "{}/rest/getAlbum?id={}&{}",
            self.base_url,
            album_id,
            self.auth_params()
        );
        let response = HTTP.get(&url).send().await?;
        Self::check_status(response.status(), "getAlbum")?;
        let text = response.text().await?;
        let resp: SubsonicAlbumResponse = serde_json::from_str(&text).map_err(|e| {
            let preview = &text[..text.len().min(200)];
            anyhow::anyhow!(
                "getAlbum({}) JSON 格式错误: {} | 预览: {}",
                album_id, e, preview
            )
        })?;
        let album = resp
            .inner
            .album
            .ok_or_else(|| anyhow::anyhow!("album not found: {}", album_id))?;

        Ok(album
            .song
            .into_iter()
            .map(|s| MusicEntry {
                id: 0,
                absolute_path: s.id,
                name: s.title,
                artist: s.artist.clone().unwrap_or_else(|| album.artist.clone()),
                album: album.name.clone(),
                // Subsonic API returns duration in seconds; convert to ms
                duration: s.duration * 1000,
                size: 0,
                server_id: String::new(),
            })
            .collect())
    }
}

// ── Subsonic API response types ──

/// Response wrapper for `getAlbumList2` (album list).
#[derive(Debug, Deserialize)]
struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    inner: SubsonicInner,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SubsonicInner {
    status: String,
    #[serde(rename = "albumList2")]
    album_list: Option<AlbumList2>,
}

#[derive(Debug, Deserialize)]
struct AlbumList2 {
    album: Vec<SubsonicAlbum>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SubsonicAlbum {
    id: String,
    name: String,
    artist: String,
    duration: u64,
    #[serde(rename = "songCount")]
    song_count: u64,
}

/// Response wrapper for `getAlbum` (album detail with songs).
#[derive(Debug, Deserialize)]
struct SubsonicAlbumResponse {
    #[serde(rename = "subsonic-response")]
    inner: SubsonicAlbumInner,
}

#[derive(Debug, Deserialize)]
struct SubsonicAlbumInner {
    album: Option<SubsonicAlbumDetail>,
}

#[derive(Debug, Deserialize)]
struct SubsonicAlbumDetail {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    artist: String,
    song: Vec<SubsonicSong>,
}

/// Response wrapper for `search3`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SubsonicSearchResponse {
    #[serde(rename = "subsonic-response")]
    inner: SubsonicSearchInner,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SubsonicSearchInner {
    #[serde(rename = "searchResult3")]
    search_result: Option<SearchResult3>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SearchResult3 {
    song: Vec<SubsonicSong>,
}

/// Response wrapper for `getLyrics`.
#[derive(Debug, Deserialize)]
struct SubsonicLyricsResponse {
    #[serde(rename = "subsonic-response")]
    inner: SubsonicLyricsInner,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SubsonicLyricsInner {
    status: String,
    lyrics: Option<SubsonicLyricsValue>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SubsonicLyricsValue {
    value: Option<String>,
}

/// A song entity as returned by Subsonic endpoints.
#[derive(Debug, Deserialize)]
struct SubsonicSong {
    id: String,
    title: String,
    #[serde(default)]
    duration: u64,
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
}

// ── Trait implementation ──

#[async_trait]
impl MusicServer for SubsonicServer {
    fn name(&self) -> &str {
        "Navidrome (Subsonic)"
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn features(&self) -> ServerFeatures {
        ServerFeatures {
            search: true,
            cover_art: true,
        }
    }

    async fn fetch_list(&self) -> Result<Vec<MusicEntry>> {
        // 1. Fetch all albums
        let url = format!(
            "{}/rest/getAlbumList2?type=alphabeticalByName&size=500&{}",
            self.base_url,
            self.auth_params()
        );
        let response = HTTP.get(&url).send().await?;
        Self::check_status(response.status(), "getAlbumList2")?;
        let text = response.text().await?;
        let resp: SubsonicResponse = serde_json::from_str(&text).map_err(|e| {
            let preview = &text[..text.len().min(200)];
            anyhow::anyhow!(
                "getAlbumList2 返回的 JSON 格式错误: {} | 预览: {}",
                e, preview
            )
        })?;
        let albums = resp.inner.album_list.map(|l| l.album).unwrap_or_default();

        // 2. Fetch songs for each album (sequential to be gentle on the server)
        let mut all_songs = Vec::new();
        for album in &albums {
            match self.fetch_album_songs(&album.id).await {
                Ok(songs) => all_songs.extend(songs),
                Err(e) => {
                    crate::log_error!("获取专辑「{}」歌曲失败: {}", album.name, e);
                }
            }
        }

        Ok(all_songs)
    }

    fn stream_url(&self, music: &MusicEntry) -> String {
        // "absolute_path" stores the Subsonic song ID
        format!(
            "{}/rest/stream?id={}&{}",
            self.base_url,
            music.absolute_path,
            self.auth_params()
        )
    }

    async fn search(&self, query: &str) -> Result<Vec<MusicEntry>> {
        let mut url = reqwest::Url::parse(&format!("{}/rest/search3", self.base_url))?;
        url.query_pairs_mut()
            .append_pair("query", query)
            .append_pair("songCount", "50")
            .append_pair("artistCount", "0")
            .append_pair("albumCount", "0")
            .append_pair("u", &self.username)
            .append_pair("p", &self.password)
            .append_pair("v", "1.16.0")
            .append_pair("c", "tune")
            .append_pair("f", "json");

        let response = HTTP.get(url).send().await?;
        Self::check_status(response.status(), "search3")?;
        let text = response.text().await?;
        let resp: SubsonicSearchResponse = serde_json::from_str(&text).map_err(|e| {
            let preview = &text[..text.len().min(200)];
            anyhow::anyhow!(
                "search3 JSON 格式错误: {} | 预览: {}",
                e, preview
            )
        })?;
        let songs = resp
            .inner
            .search_result
            .map(|r| r.song)
            .unwrap_or_default();

        Ok(songs
            .into_iter()
            .map(|s| MusicEntry {
                id: 0,
                absolute_path: s.id,
                name: s.title,
                artist: s.artist.unwrap_or_default(),
                album: s.album.unwrap_or_default(),
                // Subsonic API returns duration in seconds; convert to ms
                duration: s.duration * 1000,
                size: 0,
                server_id: String::new(),
            })
            .collect())
    }

    fn cover_url(&self, music: &MusicEntry) -> Option<String> {
        Some(format!(
            "{}/rest/getCoverArt?id={}&{}",
            self.base_url,
            music.absolute_path,
            self.auth_params()
        ))
    }

    async fn fetch_lyrics(&self, music: &MusicEntry) -> Option<Lyrics> {
        let artist = crate::server::encode_url_component(&music.artist);
        let title = crate::server::encode_url_component(&music.name);
        let url = format!(
            "{}/rest/getLyrics?artist={}&title={}&{}",
            self.base_url,
            artist,
            title,
            self.auth_params()
        );
        let response = HTTP.get(&url).send().await.ok()?;
        let text = response.text().await.ok()?;
        let resp: SubsonicLyricsResponse = serde_json::from_str(&text).ok()?;
        let value = resp.inner.lyrics?.value?;
        if value.is_empty() {
            crate::log_error!("getLyrics for «{}» returned empty", music.name);
            return None;
        }
        let parsed = crate::lyrics::parse_lyrics_text(&value);
        if parsed.is_empty() {
            crate::log_error!("getLyrics for «{}» could not parse: preview={}", music.name, &value[..value.len().min(100)]);
            return None;
        }
        // Log whether we got timed or plain lyrics
        match &parsed {
            crate::lyrics::Lyrics::Timed(lines) => {
                crate::log_error!("getLyrics for «{}»: got {} LRC lines", music.name, lines.len());
            }
            crate::lyrics::Lyrics::Plain(_) => {
                crate::log_error!("getLyrics for «{}»: got plain text (no timestamps)", music.name);
            }
        }
        Some(parsed)
    }
}
