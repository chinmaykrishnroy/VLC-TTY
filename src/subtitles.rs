//! Local subtitle support independent of terminal graphics and VLC video callbacks.
//! LibVLC selects embedded tracks; text sidecars are rendered as a Ratatui overlay.
//! SRT/WebVTT text supports Unicode BOMs and legacy Western encodings.
//! ASS layout/styling remains the native LibVLC path.
use anyhow::{bail, Context, Result};
use encoding_rs::{Encoding, WINDOWS_1252};
use std::{
    borrow::Cow,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

fn decode_text(bytes: &[u8]) -> Result<Cow<'_, str>> {
    if let Some((encoding, prefix)) = Encoding::for_bom(bytes) {
        let (text, errors) = encoding.decode_without_bom_handling(&bytes[prefix..]);
        if errors {
            bail!("Invalid {} subtitle text", encoding.name());
        }
        return Ok(text);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(Cow::Borrowed(text));
    }
    // Common legacy SRT files use Latin-1 / Windows-1252. Prefer valid
    // UTF-8 first, and never reinterpret a malformed declared Unicode BOM.
    if bytes.contains(&0) {
        bail!("Unrecognized subtitle encoding; UTF-16 needs a byte-order mark");
    }
    let (text, errors) = WINDOWS_1252.decode_without_bom_handling(bytes);
    if errors {
        bail!("Cannot decode subtitle as UTF-8 or Windows-1252");
    }
    Ok(text)
}

#[derive(Debug, Clone)]
pub struct SubtitleFile {
    pub path: PathBuf,
    pub cues: Vec<Cue>,
}

fn timestamp(input: &str) -> Option<i64> {
    let value = input.trim().replace(',', ".");
    let fields: Vec<_> = value.split(':').collect();
    if fields.len() != 2 && fields.len() != 3 {
        return None;
    }
    let seconds = fields.last()?.parse::<f64>().ok()?;
    let minutes = fields.get(fields.len() - 2)?.parse::<u64>().ok()?;
    let hours = if fields.len() == 3 {
        fields[0].parse::<u64>().ok()?
    } else {
        0
    };
    if !(0.0..60.0).contains(&seconds) || !seconds.is_finite() || minutes >= 60 {
        return None;
    }
    let whole_ms = hours
        .checked_mul(3600)?
        .checked_add(minutes.checked_mul(60)?)?
        .checked_mul(1000)?;
    i64::try_from(whole_ms)
        .ok()?
        .checked_add((seconds * 1000.0).round() as i64)
}

fn plain_text(raw: &str) -> String {
    let mut text = String::new();
    let mut inside_tag = false;
    for c in raw.chars() {
        match c {
            '<' => inside_tag = true,
            '>' if inside_tag => inside_tag = false,
            _ if !inside_tag && !c.is_control() => text.push(c),
            '\n' if !inside_tag => text.push('\n'),
            _ => {}
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

pub fn parse_text(raw: &str) -> Vec<Cue> {
    let normalized = raw
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let mut lines = block.lines();
        let first = match lines.next() {
            Some(line) => line.trim(),
            None => continue,
        };
        if first.starts_with("WEBVTT") || first.starts_with("NOTE") || first.starts_with("STYLE") {
            continue;
        }
        let timing = if first.contains("-->") {
            first
        } else {
            match lines.next() {
                Some(line) if line.contains("-->") => line,
                _ => continue,
            }
        };
        let Some((start, end)) = timing.split_once("-->") else {
            continue;
        };
        let Some(start_ms) = timestamp(start) else {
            continue;
        };
        let Some(end_ms) = timestamp(end.split_whitespace().next().unwrap_or("")) else {
            continue;
        };
        if end_ms <= start_ms {
            continue;
        }
        let text = plain_text(&lines.collect::<Vec<_>>().join("\n"));
        if !text.is_empty() {
            cues.push(Cue {
                start_ms,
                end_ms,
                text,
            });
        }
    }
    cues.sort_by_key(|c| c.start_ms);
    cues
}

impl SubtitleFile {
    pub fn load(path: &Path) -> Result<Self> {
        let mut input = fs::File::open(path)
            .with_context(|| format!("Cannot read subtitle {}", path.display()))?
            .take(5_000_001);
        let mut bytes = Vec::new();
        input
            .read_to_end(&mut bytes)
            .with_context(|| format!("Cannot read subtitle {}", path.display()))?;
        if bytes.len() > 5_000_000 {
            bail!("Subtitle file exceeds 5 MB");
        }
        let raw = decode_text(&bytes)
            .with_context(|| format!("Cannot decode subtitle {}", path.display()))?;
        let cues = parse_text(&raw);
        if cues.is_empty() {
            bail!("No readable subtitle cues in {}", path.display());
        }
        Ok(Self {
            path: path.to_owned(),
            cues,
        })
    }
    /// Most common convention: movie.en.srt, movie.srt, movie.en.vtt etc.
    pub fn discover(video: &Path) -> Option<Self> {
        let folder = video.parent()?;
        let prefix = video.file_stem()?.to_string_lossy().to_lowercase();
        let mut paths: Vec<_> = fs::read_dir(folder)
            .ok()?
            .filter_map(|x| x.ok())
            .map(|x| x.path())
            .filter(|p| {
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let ext = p
                    .extension()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                (ext == "srt" || ext == "vtt")
                    && (stem == prefix || stem.starts_with(&(prefix.clone() + ".")))
            })
            .collect();
        paths.sort_by_key(|p| {
            let s = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            (s != prefix, s)
        });
        paths.iter().find_map(|p| Self::load(p).ok())
    }
    pub fn current(&self, time_ms: i64, delay_ms: i64) -> Option<Cow<'_, str>> {
        // Positive delay means the subtitle appears later.
        let time = time_ms.saturating_sub(delay_ms);
        let i = self.cues.partition_point(|cue| cue.start_ms <= time);
        let mut active = self.cues[..i].iter().filter(|cue| time < cue.end_ms);
        let first = active.next()?;
        let mut text = Cow::Borrowed(first.text.as_str());
        for cue in active {
            text.to_mut().push('\n');
            text.to_mut().push_str(&cue.text);
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regression_legacy_and_unicode_subtitle_files_load() {
        let text =
            "1\r\n00:00:01,000 --> 00:00:03,000\r\nCaf\u{e9} \u{2014} \u{201c}hello\u{201d}\r\n";
        let legacy =
            b"1\r\n00:00:01,000 --> 00:00:03,000\r\nCaf\xe9 \x97 \x93hello\x94\r\n".to_vec();
        let mut utf8_bom = vec![0xef, 0xbb, 0xbf];
        utf8_bom.extend_from_slice(text.as_bytes());
        let mut little = vec![0xff, 0xfe];
        let mut big = vec![0xfe, 0xff];
        for unit in text.encode_utf16() {
            little.extend_from_slice(&unit.to_le_bytes());
            big.extend_from_slice(&unit.to_be_bytes());
        }
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("legacy.srt", legacy),
            ("utf8.srt", text.as_bytes().to_vec()),
            ("bom.srt", utf8_bom),
            ("little.srt", little),
            ("big.srt", big),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            let loaded = SubtitleFile::load(&path).unwrap_or_else(|e| panic!("{name}: {e:#}"));
            assert_eq!(
                loaded.current(1500, 0).as_deref(),
                Some("Caf\u{e9} \u{2014} \u{201c}hello\u{201d}")
            );
        }
    }
    #[test]
    fn declared_unicode_errors_are_not_reinterpreted_as_legacy_text() {
        for bytes in [
            b"\xef\xbb\xbf\xe9".as_slice(),
            b"\xff\xfe\x61",
            b"\xfe\xff\xd8\x00",
        ] {
            assert!(decode_text(bytes).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oversized.srt");
        fs::write(&path, vec![b' '; 5_000_001]).unwrap();
        assert!(SubtitleFile::load(&path)
            .unwrap_err()
            .to_string()
            .contains("exceeds 5 MB"));
    }

    #[test]
    fn malformed_timestamps_cannot_overflow_or_go_negative() {
        assert_eq!(timestamp("18446744073709551615:00:00.000"), None);
        assert_eq!(timestamp("00:-1.0"), None);
    }
    #[test]
    fn overlapping_cue_remains_after_shorter_cue_ends() {
        let f = SubtitleFile {
            path: "overlap.srt".into(),
            cues: parse_text(
                "1\n00:00:01,000 --> 00:00:10,000\nLong\n\n2\n00:00:02,000 --> 00:00:03,000\nShort",
            ),
        };
        assert_eq!(f.current(2500, 0).as_deref(), Some("Long\nShort"));
        assert_eq!(f.current(4000, 0).as_deref(), Some("Long"));
    }
    #[test]
    fn timestamps() {
        assert_eq!(timestamp("01:02:03,500"), Some(3_723_500));
        assert_eq!(timestamp("02:03.500"), Some(123_500));
        assert_eq!(timestamp("xx"), None);
    }
    #[test]
    fn parse_srt_and_vtt() {
        let srt = "1\r\n00:00:01,200 --> 00:00:03,000\r\n<i>Welcome!</i>\r\n\r\n2\r\n00:00:04,000 --> 00:00:07,000\r\nAgain!\r\n";
        let cues = parse_text(srt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Welcome!");
        assert_eq!(cues[1].end_ms, 7_000);
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.250 align:center\n&<b>Hi</b>\n";
        assert_eq!(parse_text(vtt)[0].text, "&Hi");
    }
    #[test]
    fn bundled_sidecar_is_discovered() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/sample.mp4");
        let track = SubtitleFile::discover(&path).expect("bundled SRT should be discoverable");
        assert!(track
            .current(1_500, 0)
            .as_deref()
            .unwrap_or("")
            .contains("TermVLC"));
        assert!(track
            .current(4_500, 0)
            .as_deref()
            .unwrap_or("")
            .contains("terminal"));
    }
    #[test]
    fn cue_lookup_and_delay() {
        let f = SubtitleFile {
            path: PathBuf::from("test.srt"),
            cues: parse_text("1\n00:00:01,000 --> 00:00:02,000\nHello"),
        };
        assert_eq!(f.current(1500, 0).as_deref(), Some("Hello"));
        assert_eq!(f.current(2000, 0).as_deref(), None);
        assert_eq!(f.current(1500, 1000).as_deref(), None);
        assert_eq!(f.current(2500, 1000).as_deref(), Some("Hello"));
    }
}
