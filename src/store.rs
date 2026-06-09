use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::api::{MusicEntry, ServerConfig};
use crate::ui::Playlist;

const CONFIG_FILE: &str = "tune_config.json";
const PLAYLISTS_FILE: &str = "tune_playlists.json";

// ── Config (multi-server) ──

/// Load all persisted server configs from disk.
/// Handles migration from old single-config format.
pub fn load_configs() -> Vec<ServerConfig> {
    let content = match std::fs::read_to_string(CONFIG_FILE) {
        Ok(c) => c,
        Err(_) => return vec![ServerConfig::default()],
    };

    // New format: JSON array
    if let Ok(configs) = serde_json::from_str::<Vec<ServerConfig>>(&content) {
        return configs;
    }

    // Old format: single object → migrate
    if let Ok(config) = serde_json::from_str::<ServerConfigOld>(&content) {
        let migrated = vec![ServerConfig {
            name: config.server_type.clone(),
            server_url: config.server_url,
            server_type: config.server_type,
            username: config.username,
            password: config.password,
            disabled: false,
        }];
        // Save migrated format
        let _ = save_configs(&migrated);
        return migrated;
    }

    vec![ServerConfig::default()]
}

pub fn save_configs(configs: &[ServerConfig]) -> Result<()> {
    let json = serde_json::to_string_pretty(configs)?;
    std::fs::write(CONFIG_FILE, json)?;
    Ok(())
}

/// Old single-server config format (for migration).
#[derive(Debug, Clone, Deserialize)]
struct ServerConfigOld {
    server_url: String,
    server_type: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
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
