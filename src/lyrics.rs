use std::io::Cursor;

use lofty::file::TaggedFileExt;
use lofty::prelude::ItemKey;
use lofty::probe::Probe;

/// A single timed lyric line.
#[derive(Debug, Clone)]
pub struct LyricLine {
    /// Timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// The line text (empty for tags like [00:12.34]).
    pub text: String,
}

/// Parsed lyrics.
#[derive(Debug, Clone)]
pub enum Lyrics {
    /// Plain text, no timestamps.
    Plain(String),
    /// Lines with millisecond timestamps.
    Timed(Vec<LyricLine>),
}

impl Lyrics {
    /// Number of lines.
    pub fn len(&self) -> usize {
        match self {
            Lyrics::Plain(t) => t.lines().count(),
            Lyrics::Timed(l) => l.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return (original, translation) at the given position.
    ///
    /// Supports two layouts:
    ///   - Two lines sharing the same timestamp → first is original, second is translation.
    ///   - A single line containing `\t` → text before tab is original, after is translation.
    pub fn lines_at(&self, position_ms: u64) -> (Option<&str>, Option<&str>) {
        match self {
            Lyrics::Plain(text) => {
                // Plain text: first line is the only content
                (text.lines().next().filter(|s| !s.is_empty()), None)
            }
            Lyrics::Timed(lines) => {
                let mut original = None;
                let mut translation = None;
                let mut current_ts = None;

                for line in lines {
                    if line.timestamp_ms > position_ms {
                        break;
                    }

                    // Handle original and translation on the same line,
                    // separated by tab (\t) or thin space (\u{2009}).
                    if let Some((left, right)) = split_orig_trans(&line.text) {
                        current_ts = Some(line.timestamp_ms);
                        original = Some(if left.is_empty() { right } else { left });
                        translation = Some(if right.is_empty() { left } else { right });
                        continue;
                    }

                    if Some(line.timestamp_ms) != current_ts {
                        // New timestamp group → reset
                        current_ts = Some(line.timestamp_ms);
                        original = Some(line.text.as_str());
                        translation = None;
                    } else {
                        // Same timestamp → translation
                        translation = Some(line.text.as_str());
                    }
                }

                (original, translation)
            }
        }
    }
}

// ── Extraction from audio bytes ──

/// Try to extract embedded lyrics from audio bytes (MP3/FLAC etc.).
///
/// This covers:
/// - ID3v2 USLT frames (MP3) → `ItemKey::Lyrics`
/// - Vorbis Comments `LYRICS=` (FLAC/OGG) → `ItemKey::Lyrics`
pub fn extract_lyrics(data: &[u8]) -> Option<Lyrics> {
    let cursor = Cursor::new(data);
    let file = Probe::new(cursor)
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;
    let tag = file.primary_tag()?;

    if let Some(text) = tag.get_string(&ItemKey::Lyrics) {
        let parsed = parse_lyrics_text(text);
        if !parsed.is_empty() {
            return Some(parsed);
        }
    }

    None
}

// ── LRC parser ──

/// Parse lyrics text that might be in LRC format or plain text.
fn parse_lyrics_text(text: &str) -> Lyrics {
    let text = text.trim();
    if text.is_empty() {
        return Lyrics::Plain(String::new());
    }

    let timed = parse_lrc_lines(text);
    if !timed.is_empty() {
        Lyrics::Timed(timed)
    } else {
        Lyrics::Plain(text.to_string())
    }
}

/// Try to parse text as LRC format.
/// Returns empty vec if no LRC timestamps found.
fn parse_lrc_lines(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // LRC: [mm:ss.xx]text or [mm:ss.xxx]text
        if let Some(rest) = line.strip_prefix('[') {
            if let Some((ts_str, remaining)) = rest.split_once(']') {
                if let Some(ms) = parse_lrc_timestamp(ts_str.trim()) {
                    let lyric_text = remaining.trim().to_string();
                    lines.push(LyricLine {
                        timestamp_ms: ms,
                        text: if lyric_text.is_empty() {
                            String::new()
                        } else {
                            lyric_text
                        },
                    });
                    continue;
                }
            }
        }

        // Also handle "[mm:ss.xx] [mm:ss.xx]text" — multiple timestamps for one line
        // This is a simplified version; multi-timestamp lines are complex.
        // For now, skip non-LRC lines
    }

    // Sort by timestamp
    if !lines.is_empty() {
        lines.sort_by_key(|l| l.timestamp_ms);
    }

    lines
}

/// Split a lyric line into (original, translation) if it contains
/// a recognised separator (tab `\t` or thin space `\u{2009}`).
fn split_orig_trans(text: &str) -> Option<(&str, &str)> {
    for sep in &['\t', '\u{2009}'] {
        if let Some((left, right)) = text.split_once(*sep) {
            let left = left.trim();
            let right = right.trim();
            if !left.is_empty() || !right.is_empty() {
                return Some((left, right));
            }
        }
    }
    None
}

fn parse_lrc_timestamp(s: &str) -> Option<u64> {
    let s = s.trim();
    // [mm:ss.xx] or [mm:ss.xxx]
    if let Some((min_str, rest)) = s.split_once(':') {
        let min: u64 = min_str.parse().ok()?;
        if let Some((sec_str, _cent_str)) = rest.split_once('.') {
            let sec: u64 = sec_str.parse().ok()?;
            // Parse centiseconds (2 or 3 digits after '.')
            let cent_str = rest.split('.').nth(1)?;
            let cent = cent_str.trim_end().parse::<u64>().ok()?;
            let cent_multiplier: u64 = if cent_str.len() >= 3 { 1 } else { 10 };
            Some(min * 60_000 + sec * 1_000 + cent * cent_multiplier)
        } else {
            // [mm:ss] without centiseconds
            let sec: u64 = rest.parse().ok()?;
            Some(min * 60_000 + sec * 1_000)
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lrc_simple() {
        let text = "[00:01.00]Line one
[00:05.50]Line two
[00:10.00]Line three";
        let lyrics = parse_lyrics_text(text);
        match lyrics {
            Lyrics::Timed(lines) => {
                assert_eq!(lines.len(), 3);
                assert_eq!(lines[0].timestamp_ms, 1000);
                assert_eq!(lines[0].text, "Line one");
                assert_eq!(lines[1].timestamp_ms, 5500);
                assert_eq!(lines[1].text, "Line two");
                assert_eq!(lines[2].timestamp_ms, 10000);
            }
            _ => panic!("Expected timed lyrics"),
        }
    }

    #[test]
    fn test_plain_text() {
        let text = "This is just\nsome plain lyrics\nwithout timestamps";
        let lyrics = parse_lyrics_text(text);
        match lyrics {
            Lyrics::Plain(t) => assert!(t.contains("plain lyrics")),
            _ => panic!("Expected plain lyrics"),
        }
    }

    #[test]
    fn test_lines_at() {
        let text = "[00:01.00]First\n[00:05.00]Second\n[00:10.00]Third\n[00:10.00]ThirdTrans";
        let lyrics = parse_lyrics_text(text);
        assert_eq!(lyrics.lines_at(0), (None, None));
        assert_eq!(lyrics.lines_at(500), (None, None));
        // First line
        let (o, t) = lyrics.lines_at(1000);
        assert_eq!(o, Some("First"));
        assert_eq!(t, None);
        let (o, t) = lyrics.lines_at(3000);
        assert_eq!(o, Some("First"));
        assert_eq!(t, None);
        // Second line
        let (o, t) = lyrics.lines_at(5000);
        assert_eq!(o, Some("Second"));
        assert_eq!(t, None);
        let (o, t) = lyrics.lines_at(9999);
        assert_eq!(o, Some("Second"));
        assert_eq!(t, None);
        // Third line with translation
        let (o, t) = lyrics.lines_at(10000);
        assert_eq!(o, Some("Third"));
        assert_eq!(t, Some("ThirdTrans"));
        let (o, t) = lyrics.lines_at(99999);
        assert_eq!(o, Some("Third"));
        assert_eq!(t, Some("ThirdTrans"));
    }

    #[test]
    fn test_tab_separated() {
        // Same line: English\tChinese — single timestamp with tab separator
        let text = "[00:01.00]First\t翻译一\n[00:05.00]Second\t翻译二";
        let lyrics = parse_lyrics_text(text);
        let (o, t) = lyrics.lines_at(1000);
        assert_eq!(o, Some("First"));
        assert_eq!(t, Some("翻译一"));
        let (o, t) = lyrics.lines_at(3000);
        assert_eq!(o, Some("First"));
        assert_eq!(t, Some("翻译一"));
        let (o, t) = lyrics.lines_at(5000);
        assert_eq!(o, Some("Second"));
        assert_eq!(t, Some("翻译二"));
    }

    #[test]
    fn test_split_orig_trans() {
        // Tab separator
        let (l, r) = split_orig_trans("Hello\t世界").unwrap();
        assert_eq!(l, "Hello");
        assert_eq!(r, "世界");
        // Thin space separator
        let (l, r) = split_orig_trans("Hello\u{2009}世界").unwrap();
        assert_eq!(l, "Hello");
        assert_eq!(r, "世界");
        // No separator
        assert!(split_orig_trans("Hello World").is_none());
    }

    #[test]
    fn test_thin_space_separator() {
        // Real-world format: English\u{2009}Chinese
        let text = "[00:17.00]It being in the spring time\u{2009}那是在春天的时节";
        let lyrics = parse_lyrics_text(text);
        let (o, t) = lyrics.lines_at(17000);
        assert_eq!(o, Some("It being in the spring time"));
        assert_eq!(t, Some("那是在春天的时节"));
    }

}
