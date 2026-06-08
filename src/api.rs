use anyhow::Result;
use serde::{Deserialize, Serialize};

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

/// Fetch music list from the server API.
pub async fn fetch_music_list(base_url: &str) -> Result<Vec<MusicEntry>> {
    let url = format!("{}/musicsV2", base_url);
    let resp: MusicListResponse = reqwest::get(&url).await?.json().await?;
    Ok(resp.children)
}

/// Build the playable URL for a music file.
pub fn get_music_url(base_url: &str, absolute_path: &str) -> String {
    let encoded_path = encode_url_path(absolute_path);
    format!("{}/file?path={}", base_url, encoded_path)
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
