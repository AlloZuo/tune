// ── Online lyrics provider (LRCLIB API) ──
//
// Searches lrclib.net for timed/synced lyrics by song title and artist,
// then picks the best match using duration proximity.
//
// LRCLIB is free, open, and requires no API key.
// Docs: https://lrclib.net/docs

use serde::Deserialize;

use crate::lyrics::Lyrics;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LrcLibSearchResult {
    id: u64,
    #[serde(rename = "trackName")]
    track_name: String,
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(rename = "albumName", default)]
    album_name: Option<String>,
    /// Duration **in seconds**.
    duration: f64,
    #[serde(default)]
    instrumental: bool,
    #[serde(rename = "plainLyrics", default)]
    plain_lyrics: Option<String>,
    #[serde(rename = "syncedLyrics", default)]
    synced_lyrics: Option<String>,
}

/// Search LRCLIB for lyrics matching `title` and `artist`, picking the best
/// result by duration proximity to `duration_ms`.
///
/// Returns `None` when:
/// - The network request fails (timeout, DNS, etc.)
/// - No matching (non-instrumental) results are found
/// - The matched result has no lyrics text
///
/// This function is designed to be called as a **fallback** — it is safe
/// to call unconditionally since it handles all errors gracefully.
pub async fn search(title: &str, artist: &str, duration_ms: u64) -> Option<Lyrics> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let artist = artist.trim();

    // ── 1. Search LRCLIB ──
    let results: Vec<LrcLibSearchResult> = crate::server::HTTP
        .get("https://lrclib.net/api/search")
        .query(&[
            ("track_name", title),
            ("artist_name", if artist.is_empty() { title } else { artist }),
        ])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    if results.is_empty() {
        return None;
    }

    // ── 2. Score each result and pick the best match ──
    //   - Non-instrumental strongly preferred (100_000 penalty for instrumental)
    //   - Duration proximity: ±1 s → 0 penalty, up to ±5 s → scaled penalty
    //   - Among ties, the first one wins (stable sort).
    let target_secs = (duration_ms as f64) / 1000.0;
    let has_duration = duration_ms > 0;

    let best = results.iter().min_by_key(|r| {
        let instrumental_penalty = if r.instrumental { 100_000u64 } else { 0 };

        let duration_penalty = if has_duration {
            let diff = (r.duration - target_secs).abs();
            if diff < 1.0 {
                0u64
            } else if diff < 5.0 {
                (diff * 100.0) as u64
            } else {
                // Beyond 5 s difference is a strong mismatch signal
                50_000u64
            }
        } else {
            0
        };

        instrumental_penalty + duration_penalty
    })?;

    // ── 3. Extract lyrics text (synced LRC preferred) ──
    let lyrics_text = best
        .synced_lyrics
        .as_deref()
        .or(best.plain_lyrics.as_deref())?;

    let parsed = crate::lyrics::parse_lyrics_text(lyrics_text);
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    /// LRCLIB search by title + artist should return results for well-known songs.
    ///
    /// NOTE: This is an integration test that depends on the network.
    /// It is marked with `#[ignore]` so it only runs when explicitly requested:
    ///     cargo test -- --ignored lyrics_online
    ///
    /// Run it once after implementing to verify the API is reachable from your
    /// network and that the response format matches our deserialisation.
    #[tokio::test]
    #[ignore]
    async fn test_search_known_song() {
        // "Hello" by Adele (4:56 = 296s) is confirmed present in LRCLIB
        let lyrics = search("Hello", "Adele", 296_000).await;
        assert!(lyrics.is_some(), "should find lyrics for Hello by Adele");
        if let Some(Lyrics::Timed(lines)) = lyrics {
            assert!(!lines.is_empty(), "should have timed lines");
            assert!(
                lines.last().unwrap().timestamp_ms >= 200_000,
                "timed lyrics should cover most of the ~4:56 song"
            );
        } else {
            panic!("expected Timed lyrics, got Plain or None");
        }
    }

    /// Empty title should always return None (no network call).
    #[tokio::test]
    async fn test_empty_title() {
        assert!(search("", "Some Artist", 0).await.is_none());
    }
}
