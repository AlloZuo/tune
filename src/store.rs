use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::api::MusicEntry;
use crate::ui::Playlist;

const CONFIG_FILE: &str = "tune_config.json";
const PLAYLISTS_FILE: &str = "tune_playlists.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: String::new(),
        }
    }
}

// ── Config ──

pub fn load_config() -> Config {
    std::fs::read_to_string(CONFIG_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(config: &Config) -> Result<()> {
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(CONFIG_FILE, json)?;
    Ok(())
}

// ── Playlists ──

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlaylistJson {
    name: String,
    songs: Vec<MusicEntry>,
}

pub fn load_playlists() -> Vec<Playlist> {
    let raw: Vec<PlaylistJson> = std::fs::read_to_string(PLAYLISTS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    raw.into_iter()
        .map(|pj| Playlist {
            name: pj.name,
            songs: pj.songs,
        })
        .collect()
}

pub fn save_playlists(playlists: &[Playlist]) -> Result<()> {
    let raw: Vec<PlaylistJson> = playlists
        .iter()
        .map(|p| PlaylistJson {
            name: p.name.clone(),
            songs: p.songs.clone(),
        })
        .collect();
    let json = serde_json::to_string_pretty(&raw)?;
    std::fs::write(PLAYLISTS_FILE, json)?;
    Ok(())
}
