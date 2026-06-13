use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::server::{MusicEntry, ServerConfig};
use crate::ui::Playlist;

/// Debounce interval for config writes — rapid changes coalesce into one write.
const DEBOUNCE_MS: Duration = Duration::from_millis(200);

/// True when a deferred write is already scheduled.
static SAVE_PENDING: AtomicBool = AtomicBool::new(false);

/// Single unified config file — replaces the old separate files for servers,
/// playlists, and language.
const CONFIG_FILE: &str = "tune_config.json";

// ── In-memory config cache ──
//
// Loaded once at startup (with migrations), then all load/save operations
// read/write this cache. On-disk writes happen on every mutation for
// crash-safety but reads are zero-I/O.

static CONFIG: LazyLock<RwLock<TuneConfig>> = LazyLock::new(|| {
    RwLock::new(load_config_from_disk())
});

// ── Unified config structure ──

/// Last playback position (for "remember playback position" feature).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LastPlayed {
    pub server_id: String,
    pub absolute_path: String,
    /// Position in milliseconds to resume from.
    #[serde(default)]
    pub position_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuneConfig {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub playlists: Vec<PlaylistJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<LastPlayed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_volume: Option<f64>,
}

/// Load the config from disk (with migrations). Called once at startup.
/// Never fails — returns defaults on any error.
fn load_config_from_disk() -> TuneConfig {
    let content = match std::fs::read_to_string(CONFIG_FILE) {
        Ok(c) => c,
        Err(_) => {
            // No config file yet → try legacy files then return default
            return migrate_from_legacy();
        }
    };

    // 1) New unified format
    if let Ok(cfg) = serde_json::from_str::<TuneConfig>(&content) {
        return cfg;
    }

    // 2) Old format: JSON array of ServerConfig → migrate
    if let Ok(servers) = serde_json::from_str::<Vec<ServerConfig>>(&content) {
        let mut cfg = TuneConfig {
            language: "zh".to_string(),
            servers,
            playlists: Vec::new(),
            last_played: None,
            default_volume: None,
        };
        merge_legacy_playlists(&mut cfg);
        merge_legacy_lang(&mut cfg);
        let _ = save_config_inner(&cfg);
        return cfg;
    }

    // 3) Old format: single ServerConfigOld object → migrate
    #[derive(Debug, Clone, Deserialize)]
    struct ServerConfigOld {
        server_url: String,
        server_type: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    }
    if let Ok(old) = serde_json::from_str::<ServerConfigOld>(&content) {
        let cfg = TuneConfig {
            language: "zh".to_string(),
            servers: vec![ServerConfig {
                name: old.server_type.clone(),
                server_url: old.server_url,
                server_type: old.server_type,
                username: old.username,
                password: old.password,
                disabled: false,
            }],
            playlists: Vec::new(),
            last_played: None,
            default_volume: None,
        };
        let _ = save_config_inner(&cfg);
        return cfg;
    }

    // Fallback
    TuneConfig::default()
}

fn save_config_inner(cfg: &TuneConfig) -> Result<()> {
    let json = serde_json::to_string_pretty(cfg)?;
    // Write to temp file, then rename — prevents corruption on crash.
    let tmp = format!("{}.tmp", CONFIG_FILE);
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, CONFIG_FILE)?;
    Ok(())
}

/// Try to read legacy `tune_playlists.json` and merge into the unified config.
const LEGACY_PLAYLISTS: &str = "tune_playlists.json";
const LEGACY_LANG: &str = "tune_lang.json";

fn migrate_from_legacy() -> TuneConfig {
    let mut cfg = TuneConfig::default();
    merge_legacy_playlists(&mut cfg);
    merge_legacy_lang(&mut cfg);
    // Save immediately so next start reads the new format.
    let _ = save_config_inner(&cfg);
    cfg
}

fn merge_legacy_playlists(cfg: &mut TuneConfig) {
    let raw: Vec<PlaylistJsonLegacy> = std::fs::read_to_string(LEGACY_PLAYLISTS)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    cfg.playlists = raw
        .into_iter()
        .map(|pj| PlaylistJson {
            name: pj.name,
            songs: pj.songs,
        })
        .collect();
    // Clean up legacy file
    let _ = std::fs::remove_file(LEGACY_PLAYLISTS);
}

fn merge_legacy_lang(cfg: &mut TuneConfig) {
    let lang = std::fs::read_to_string(LEGACY_LANG)
        .ok()
        .map(|s| {
            let s = s.trim().trim_matches('"').to_string();
            if s == "en" || s == "English" {
                "en".to_string()
            } else {
                "zh".to_string()
            }
        })
        .unwrap_or_else(|| "zh".to_string());
    cfg.language = lang;
    let _ = std::fs::remove_file(LEGACY_LANG);
}

// ── Playlist JSON (used inside TuneConfig) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistJson {
    pub name: String,
    pub songs: Vec<MusicEntry>,
}

/// Legacy playlist format (same shape, different name for clarity).
#[derive(Debug, Clone, Deserialize)]
struct PlaylistJsonLegacy {
    name: String,
    songs: Vec<MusicEntry>,
}

// ── Public API ──

impl Default for TuneConfig {
    fn default() -> Self {
        Self {
            language: "zh".to_string(),
            servers: vec![ServerConfig::default()],
            playlists: Vec::new(),
            last_played: None,
            default_volume: None,
        }
    }
}

const DEFAULT_VOLUME: f64 = 0.8;

pub fn load_volume() -> f32 {
    CONFIG
        .read()
        .expect("CONFIG lock poisoned")
        .default_volume
        .unwrap_or(DEFAULT_VOLUME) as f32
}

/// Schedule a debounced config write. Multiple calls within `DEBOUNCE_MS`
/// coalesce into a single write. The in-memory config is always up-to-date;
/// only disk persistence is deferred.
fn schedule_save() {
    if SAVE_PENDING.swap(true, Ordering::AcqRel) {
        return; // A deferred write is already scheduled
    }
    tokio::spawn(async move {
        tokio::time::sleep(DEBOUNCE_MS).await;
        SAVE_PENDING.store(false, Ordering::Release);
        let cfg = CONFIG.read().expect("CONFIG lock poisoned");
        let _ = save_config_inner(&cfg);
    });
}

/// Force an immediate config write for any pending changes. Call on quit to
/// ensure no data loss.
pub fn flush_config() {
    if !SAVE_PENDING.load(Ordering::Acquire) {
        return;
    }
    SAVE_PENDING.store(false, Ordering::Release);
    let cfg = CONFIG.read().expect("CONFIG lock poisoned");
    let _ = save_config_inner(&cfg);
}

pub fn save_volume(vol: f32) -> Result<()> {
    let mut cfg = CONFIG.write().expect("CONFIG lock poisoned");
    cfg.default_volume = Some((vol as f64).clamp(0.0, 1.0));
    schedule_save();
    Ok(())
}

pub fn load_servers() -> Vec<ServerConfig> {
    CONFIG.read().expect("CONFIG lock poisoned").servers.clone()
}

pub fn save_servers(servers: &[ServerConfig]) -> Result<()> {
    let mut cfg = CONFIG.write().expect("CONFIG lock poisoned");
    cfg.servers = servers.to_vec();
    schedule_save();
    Ok(())
}

pub fn load_playlists() -> Vec<Playlist> {
    CONFIG
        .read()
        .expect("CONFIG lock poisoned")
        .playlists
        .iter()
        .map(|pj| Playlist {
            name: pj.name.clone(),
            songs: pj.songs.clone(),
        })
        .collect()
}

pub fn save_playlists(playlists: &[Playlist]) -> Result<()> {
    let mut cfg = CONFIG.write().expect("CONFIG lock poisoned");
    cfg.playlists = playlists
        .iter()
        .map(|p| PlaylistJson {
            name: p.name.clone(),
            songs: p.songs.clone(),
        })
        .collect();
    schedule_save();
    Ok(())
}

pub fn load_language() -> String {
    CONFIG.read().expect("CONFIG lock poisoned").language.clone()
}

pub fn save_language(lang: &str) -> Result<()> {
    let mut cfg = CONFIG.write().expect("CONFIG lock poisoned");
    cfg.language = lang.to_string();
    schedule_save();
    Ok(())
}

pub fn load_last_played() -> Option<LastPlayed> {
    CONFIG.read().expect("CONFIG lock poisoned").last_played.clone()
}

pub fn save_last_played(last: &LastPlayed) -> Result<()> {
    let mut cfg = CONFIG.write().expect("CONFIG lock poisoned");
    cfg.last_played = Some(last.clone());
    schedule_save();
    Ok(())
}

pub fn clear_last_played() -> Result<()> {
    let mut cfg = CONFIG.write().expect("CONFIG lock poisoned");
    cfg.last_played = None;
    schedule_save();
    Ok(())
}