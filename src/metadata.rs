//! Audio metadata and album artwork extraction.
//! Supports ID3v2 (MP3/WAV/AIFF), FLAC (VORBIS_COMMENT + PICTURE), MP4/M4A/AAC (covr atom),
//! OGG/OPUS, directory cover art fallbacks (cover.jpg/png, folder.jpg/png), and universal image scan.

use crate::vlc::VideoFrame;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "wav", "m4a", "aac", "opus", "wma", "ape", "alac", "aiff", "aif", "mka",
    "mid", "midi", "ac3", "dts", "ra", "m4b", "m4p", "oga",
];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

#[derive(Clone, Debug, Default)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub thumbnail: Option<Arc<VideoFrame>>,
}

/// Metadata discovered by LibVLC's demuxer, including for network media.
#[derive(Clone, Debug, Default)]
pub struct MediaMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
}

fn percent_decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut input = text.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = char::from(input.next()?).to_digit(16)?;
            let low = char::from(input.next()?).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

/// Decode only a URL's final path component. Local percent signs and URL '+'
/// characters are literal; query strings must not become part of the title.
pub fn display_name(source: &str) -> String {
    if source.contains("://") {
        let path = source.split(['?', '#']).next().unwrap_or(source);
        let name = path.rsplit('/').next().unwrap_or(path);
        percent_decode(name).unwrap_or_else(|| name.to_owned())
    } else {
        source
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(source)
            .to_owned()
    }
}

pub fn is_audio_source(source: &str) -> bool {
    is_audio_file(Path::new(&display_name(source)))
}

/// LibVLC versions differ on whether a provisional MRL title is decoded and
/// whether it retains the query string. None should replace actual track tags.
pub fn is_url_title(source: &str, title: &str) -> bool {
    if !source.contains("://") {
        return false;
    }
    let clean = source.split(['?', '#']).next().unwrap_or(source);
    [
        source,
        clean,
        source.rsplit('/').next().unwrap_or(source),
        clean.rsplit('/').next().unwrap_or(clean),
    ]
    .into_iter()
    .any(|candidate| title == candidate || percent_decode(candidate).as_deref() == Some(title))
}

fn artwork_path(url: &str) -> Option<std::path::PathBuf> {
    let raw = url.strip_prefix("file://")?;
    // Do not follow remote file authorities or network shares from media tags.
    let raw = raw
        .strip_prefix("localhost/")
        .map(|p| format!("/{p}"))
        .unwrap_or_else(|| raw.to_owned());
    if !raw.starts_with('/') || raw.starts_with("//") {
        return None;
    }
    let path = percent_decode(&raw)?;
    if path.starts_with("//") || path.starts_with("/\\") || path.starts_with("\\\\") {
        return None;
    }
    #[cfg(windows)]
    let path = if path.as_bytes().get(2) == Some(&b':') {
        path[1..].to_owned()
    } else {
        path
    };
    Some(path.into())
}

type ArtworkResult = (u64, String, Option<AudioMetadata>);
enum ArtworkJob {
    File(std::path::PathBuf),
    Id3(String),
}

/// Decode cached artwork off the UI thread with one replaceable pending job.
/// Media revisions prevent late results from replacing the next track's cover.
#[derive(Default)]
pub struct ArtworkLoader {
    requests: Option<crate::latest::Sender<(u64, String, ArtworkJob)>>,
    results: Option<crate::latest::Receiver<ArtworkResult>>,
    requested: Option<(u64, String)>,
}

impl std::fmt::Debug for ArtworkLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtworkLoader")
            .field("pending", &self.requested.is_some())
            .finish()
    }
}

impl ArtworkLoader {
    pub fn request(&mut self, revision: u64, url: &str) {
        if self
            .requested
            .as_ref()
            .is_some_and(|(r, u)| *r == revision && u == url)
        {
            return;
        }
        let job = artwork_path(url).map(ArtworkJob::File).or_else(|| {
            (url.starts_with("http://") && display_name(url).to_ascii_lowercase().ends_with(".mp3"))
                .then(|| ArtworkJob::Id3(url.to_owned()))
        });
        let Some(job) = job else {
            return;
        };
        if self.requests.is_none() {
            let (requests, incoming) = crate::latest::channel::<(u64, String, ArtworkJob)>();
            let (outgoing, results) = crate::latest::channel();
            if std::thread::Builder::new()
                .name("album-art".into())
                .spawn(move || {
                    while let Some((revision, url, job)) = incoming.recv() {
                        let image = match job {
                            ArtworkJob::File(path) => {
                                load_artwork(&path).map(|thumbnail| AudioMetadata {
                                    thumbnail: Some(thumbnail),
                                    ..Default::default()
                                })
                            }
                            ArtworkJob::Id3(url) => fetch_id3_art(&url),
                        };
                        if outgoing.replace((revision, url, image)).is_err() {
                            break;
                        }
                    }
                })
                .is_err()
            {
                return;
            }
            self.requests = Some(requests);
            self.results = Some(results);
        }
        self.requested = Some((revision, url.to_owned()));
        if let Some(requests) = &self.requests {
            let _ = requests.replace((revision, url.to_owned(), job));
        }
    }

    pub fn poll(&mut self, revision: u64) -> Option<AudioMetadata> {
        let (received, url, image) = self.results.as_ref()?.try_take()?;
        if url.starts_with("file://") && self.requested.as_ref() == Some(&(received, url)) {
            self.requested = None; // Retry if VLC has not finished writing its cache file yet.
        }
        (received == revision).then_some(image).flatten()
    }
}

fn fetch_id3_art(url: &str) -> Option<AudioMetadata> {
    let agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(3)))
        .build()
        .new_agent();
    let range = |end: usize| -> Option<Vec<u8>> {
        let mut response = agent
            .get(url)
            .header("Range", format!("bytes=0-{end}"))
            .call()
            .ok()?;
        // Never accept an unbounded full-file response from a server ignoring Range.
        if response.status().as_u16() != 206 {
            return None;
        }
        let content_range = response.headers().get("content-range")?.to_str().ok()?;
        if !content_range.starts_with(&format!("bytes 0-{end}/")) {
            return None;
        }
        let bytes = response
            .body_mut()
            .with_config()
            // Allow the reader to observe EOF after the exact expected body.
            // The length check below still rejects an extra byte.
            .limit((end + 2) as u64)
            .read_to_vec()
            .ok()?;
        (bytes.len() == end + 1).then_some(bytes)
    };
    let header = range(9)?;
    if !header.starts_with(b"ID3") || header[6..10].iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    let size = synchsafe_u32(&header[6..10]) as usize;
    if size == 0 || size > 16 * 1024 * 1024 {
        return None;
    }
    let bytes = range(9 + size)?;
    let mut meta = AudioMetadata::default();
    parse_id3v2(&bytes, &mut meta);
    Some(meta)
}

fn load_artwork(path: &Path) -> Option<Arc<VideoFrame>> {
    let file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() || file.metadata().ok()?.len() > 16 * 1024 * 1024 {
        return None;
    }
    decode_artwork(std::io::BufReader::new(file))
}

fn decode_artwork(reader: impl std::io::BufRead + Seek) -> Option<Arc<VideoFrame>> {
    let mut reader = image::ImageReader::new(reader).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Some(image_to_video_frame(reader.decode().ok()?))
}

/// Convert an image::DynamicImage into an owned VideoFrame for TUI rendering
pub fn image_to_video_frame(img: image::DynamicImage) -> Arc<VideoFrame> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    for (src, dst) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(bgra.as_chunks_mut::<4>().0)
    {
        dst[0] = src[2]; // B
        dst[1] = src[1]; // G
        dst[2] = src[0]; // R
        dst[3] = src[3]; // A
    }
    Arc::new(VideoFrame {
        width: w as usize,
        height: h as usize,
        pitch: (w * 4) as usize,
        bgra,
        serial: 1,
    })
}

/// Extract metadata and album artwork from an audio file.
pub fn extract_metadata(path: &Path) -> AudioMetadata {
    let mut meta = AudioMetadata::default();
    let Ok(mut file) = File::open(path) else {
        return meta;
    };

    // Read up to 16 MB of tag data (covers are almost always in the first few MB)
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let read_len = (file_len.min(16 * 1024 * 1024)) as usize;
    let mut buffer = vec![0u8; read_len];
    if file.read_exact(&mut buffer).is_err() {
        let _ = file.seek(SeekFrom::Start(0));
        buffer.clear();
        let _ = file.take(16 * 1024 * 1024).read_to_end(&mut buffer);
    }

    // 1. Try format-specific parser
    if buffer.starts_with(b"ID3") {
        parse_id3v2(&buffer, &mut meta);
    } else if buffer.starts_with(b"fLaC") {
        parse_flac(&buffer, &mut meta);
    } else if buffer.windows(4).any(|w| w == b"ftyp" || w == b"moov") {
        parse_mp4_m4a(&buffer, &mut meta);
    }

    // 2. If thumbnail wasn't found from structured tags, scan for JPEG / PNG magic in buffer
    if meta.thumbnail.is_none() && buffer.len() > 64 {
        if let Some(art) = scan_magic_image(&buffer) {
            meta.thumbnail = Some(art);
        }
    }

    // 3. Fallback: Check folder for cover.jpg, cover.png, folder.jpg, etc.
    if meta.thumbnail.is_none() {
        if let Some(folder) = path.parent() {
            meta.thumbnail = find_folder_artwork(folder, path);
        }
    }

    meta
}

fn scan_magic_image(buf: &[u8]) -> Option<Arc<VideoFrame>> {
    // Look for JPEG SOI (0xFF, 0xD8, 0xFF)
    for i in 0..buf.len().saturating_sub(64) {
        if (buf[i] == 0xFF && buf[i + 1] == 0xD8 && buf[i + 2] == 0xFF)
            || buf[i..].starts_with(b"\x89PNG\r\n\x1a\n")
        {
            if let Some(img) = decode_artwork(std::io::Cursor::new(&buf[i..])) {
                if img.width >= 32 && img.height >= 32 {
                    return Some(img);
                }
            }
        }
    }
    None
}

fn find_folder_artwork(folder: &Path, media_path: &Path) -> Option<Arc<VideoFrame>> {
    let stem = media_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let candidates = [
        format!("{stem}.jpg"),
        format!("{stem}.png"),
        format!("{stem}.jpeg"),
        "cover.jpg".into(),
        "cover.png".into(),
        "cover.jpeg".into(),
        "folder.jpg".into(),
        "folder.png".into(),
        "folder.jpeg".into(),
        "album.jpg".into(),
        "album.png".into(),
        "front.jpg".into(),
        "front.png".into(),
    ];

    for name in &candidates {
        let p = folder.join(name);
        if p.is_file() {
            if let Ok(img) = image::open(&p) {
                return Some(image_to_video_frame(img));
            }
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────
// ID3v2 Parser (MP3, AIFF, WAV)
// ──────────────────────────────────────────────────────────────────────
fn parse_id3v2(data: &[u8], meta: &mut AudioMetadata) {
    if data.len() < 10 {
        return;
    }
    let version = data[3];
    let tag_size = synchsafe_u32(&data[6..10]) as usize;
    let end = (10 + tag_size).min(data.len());
    let mut offset = 10;

    while offset + 10 < end {
        let (id, frame_size, header_len) = if version == 2 {
            // ID3v2.2 uses 3-byte ID and 3-byte size
            if offset + 6 > end {
                break;
            }
            let id = std::str::from_utf8(&data[offset..offset + 3]).unwrap_or("");
            let sz = ((data[offset + 3] as usize) << 16)
                | ((data[offset + 4] as usize) << 8)
                | (data[offset + 5] as usize);
            (id, sz, 6)
        } else {
            // ID3v2.3 and ID3v2.4 use 4-byte ID and 4-byte size
            let id = std::str::from_utf8(&data[offset..offset + 4]).unwrap_or("");
            let sz = if version == 4 {
                synchsafe_u32(&data[offset + 4..offset + 8]) as usize
            } else {
                u32_be(&data[offset + 4..offset + 8]) as usize
            };
            (id, sz, 10)
        };

        if frame_size == 0 || offset + header_len + frame_size > end {
            break;
        }
        let payload = &data[offset + header_len..offset + header_len + frame_size];

        match id {
            "TIT2" | "TT2" => {
                if meta.title.is_none() {
                    meta.title = decode_id3_text(payload);
                }
            }
            "TPE1" | "TP1" => {
                if meta.artist.is_none() {
                    meta.artist = decode_id3_text(payload);
                }
            }
            "TALB" | "TAL" => {
                if meta.album.is_none() {
                    meta.album = decode_id3_text(payload);
                }
            }
            "APIC" | "PIC" if meta.thumbnail.is_none() => {
                if let Some(art) = parse_id3_apic(payload, version) {
                    meta.thumbnail = Some(art);
                }
            }
            _ => {}
        }
        offset += header_len + frame_size;
    }
}

fn parse_id3_apic(payload: &[u8], version: u8) -> Option<Arc<VideoFrame>> {
    if payload.len() < 5 {
        return None;
    }
    let mut idx = 1; // skip encoding byte
    if version == 2 {
        idx += 3; // 3-byte image format (e.g. "JPG", "PNG")
        idx += 1; // picture type
        while idx < payload.len() && payload[idx] != 0 {
            idx += 1;
        }
        idx += 1; // skip null
    } else {
        // MIME type is null-terminated ASCII
        while idx < payload.len() && payload[idx] != 0 {
            idx += 1;
        }
        idx += 1; // skip null
        if idx >= payload.len() {
            return None;
        }
        idx += 1; // skip picture type
                  // Skip description (null terminated, either single or double 0)
        while idx < payload.len() && payload[idx] != 0 {
            idx += 1;
        }
        if idx < payload.len() && payload[idx] == 0 {
            idx += 1;
        }
        if idx < payload.len() && payload[idx] == 0 {
            idx += 1;
        }
    }

    if idx < payload.len() {
        let img_bytes = &payload[idx..];
        if let Some(img) = decode_artwork(std::io::Cursor::new(img_bytes)) {
            return Some(img);
        }
    }
    // Fallback: look for JPEG/PNG header in the frame payload directly
    scan_magic_image(payload)
}

fn decode_id3_text(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    let enc = payload[0];
    let body = &payload[1..];
    match enc {
        0 => Some(
            body.iter()
                .map(|&b| char::from(b))
                .collect::<String>()
                .trim_matches('\0')
                .to_owned(),
        ),
        1 | 2 => {
            let (little_endian, body) = if enc == 2 {
                (false, body)
            } else if body.starts_with(&[0xFF, 0xFE]) {
                (true, &body[2..])
            } else if body.starts_with(&[0xFE, 0xFF]) {
                (false, &body[2..])
            } else {
                return None;
            };
            if body.len() % 2 != 0 {
                return None;
            }
            let u16s: Vec<u16> = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if little_endian {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .collect();
            String::from_utf16(&u16s)
                .ok()
                .map(|s| s.trim_matches('\0').to_string())
        }
        3 => String::from_utf8(body.to_vec())
            .ok()
            .map(|s| s.trim_matches('\0').to_string()),
        _ => None,
    }
    .filter(|s| !s.trim().is_empty())
}

fn synchsafe_u32(b: &[u8]) -> u32 {
    ((b[0] as u32 & 0x7F) << 21)
        | ((b[1] as u32 & 0x7F) << 14)
        | ((b[2] as u32 & 0x7F) << 7)
        | (b[3] as u32 & 0x7F)
}

fn u32_be(b: &[u8]) -> u32 {
    ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32)
}

// ──────────────────────────────────────────────────────────────────────
// FLAC Parser (VORBIS_COMMENT and PICTURE metadata blocks)
// ──────────────────────────────────────────────────────────────────────
fn parse_flac(data: &[u8], meta: &mut AudioMetadata) {
    if data.len() < 4 || &data[0..4] != b"fLaC" {
        return;
    }
    let mut offset = 4;
    while offset + 4 <= data.len() {
        let header = data[offset];
        let is_last = (header & 0x80) != 0;
        let block_type = header & 0x7F;
        let length = ((data[offset + 1] as usize) << 16)
            | ((data[offset + 2] as usize) << 8)
            | (data[offset + 3] as usize);
        offset += 4;
        if offset + length > data.len() {
            break;
        }
        let block = &data[offset..offset + length];

        if block_type == 4 {
            // VORBIS_COMMENT
            parse_vorbis_comments(block, meta);
        } else if block_type == 6 && meta.thumbnail.is_none() {
            // METADATA_BLOCK_PICTURE
            if let Some(art) = parse_flac_picture_block(block) {
                meta.thumbnail = Some(art);
            }
        }

        offset += length;
        if is_last {
            break;
        }
    }
}

fn parse_flac_picture_block(data: &[u8]) -> Option<Arc<VideoFrame>> {
    if data.len() < 32 {
        return None;
    }
    let mime_len = u32_be(&data[4..8]) as usize;
    let mut pos = 8 + mime_len;
    if pos + 4 > data.len() {
        return None;
    }
    let desc_len = u32_be(&data[pos..pos + 4]) as usize;
    pos += 4 + desc_len;
    pos += 16; // width (4), height (4), depth (4), colors (4)
    if pos + 4 > data.len() {
        return None;
    }
    let data_len = u32_be(&data[pos..pos + 4]) as usize;
    pos += 4;
    if pos + data_len <= data.len() {
        if let Ok(img) = image::load_from_memory(&data[pos..pos + data_len]) {
            return Some(image_to_video_frame(img));
        }
    }
    scan_magic_image(data)
}

fn parse_vorbis_comments(block: &[u8], meta: &mut AudioMetadata) {
    if block.len() < 8 {
        return;
    }
    let vendor_len = u32_be(&[block[3], block[2], block[1], block[0]]) as usize;
    let mut pos = 4 + vendor_len;
    if pos + 4 > block.len() {
        return;
    }
    let count = u32_be(&[block[pos + 3], block[pos + 2], block[pos + 1], block[pos]]) as usize;
    pos += 4;

    for _ in 0..count.min(200) {
        if pos + 4 > block.len() {
            break;
        }
        let len = u32_be(&[block[pos + 3], block[pos + 2], block[pos + 1], block[pos]]) as usize;
        pos += 4;
        if pos + len > block.len() {
            break;
        }
        if let Ok(comment) = std::str::from_utf8(&block[pos..pos + len]) {
            if let Some((k, v)) = comment.split_once('=') {
                match k.to_ascii_uppercase().as_str() {
                    "TITLE" if meta.title.is_none() => meta.title = Some(v.to_string()),
                    "ARTIST" if meta.artist.is_none() => meta.artist = Some(v.to_string()),
                    "ALBUM" if meta.album.is_none() => meta.album = Some(v.to_string()),
                    _ => {}
                }
            }
        }
        pos += len;
    }
}

// ──────────────────────────────────────────────────────────────────────
// MP4 / M4A / AAC Parser (moov -> udta -> meta -> ilst -> covr)
// ──────────────────────────────────────────────────────────────────────
fn parse_mp4_m4a(data: &[u8], meta: &mut AudioMetadata) {
    // Search for covr atom
    if meta.thumbnail.is_none() {
        if let Some(pos) = data.windows(4).position(|w| w == b"covr") {
            // inside covr, search for "data"
            let slice = &data[pos..data.len().min(pos + 1024 * 1024 * 8)];
            if let Some(dpos) = slice.windows(4).position(|w| w == b"data") {
                let img_start = dpos + 12; // 4 size, 4 'data', 4 flags
                if img_start < slice.len() {
                    if let Ok(img) = image::load_from_memory(&slice[img_start..]) {
                        meta.thumbnail = Some(image_to_video_frame(img));
                    }
                }
            }
        }
    }

    // Search for Title (\xa9nam), Artist (\xa9ART), Album (\xa9alb)
    let tags = [(b"\xa9nam", 1), (b"\xa9ART", 2), (b"\xa9alb", 3)];
    for (tag, kind) in tags {
        if let Some(pos) = data.windows(4).position(|w| w == tag) {
            let slice = &data[pos..data.len().min(pos + 512)];
            if let Some(dpos) = slice.windows(4).position(|w| w == b"data") {
                let text_start = dpos + 12;
                if text_start < slice.len() {
                    let end = slice[text_start..]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(slice.len() - text_start);
                    if let Ok(text) = std::str::from_utf8(&slice[text_start..text_start + end]) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            match kind {
                                1 if meta.title.is_none() => meta.title = Some(trimmed.to_string()),
                                2 if meta.artist.is_none() => {
                                    meta.artist = Some(trimmed.to_string())
                                }
                                3 if meta.album.is_none() => meta.album = Some(trimmed.to_string()),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_display_names_decode_once_without_changing_local_names() {
        assert_eq!(
            display_name("http://localhost/token/a%20b%23%C3%A9+z.mp3?token=secret#fragment"),
            "a b#é+z.mp3"
        );
        assert_eq!(display_name("http://localhost/a%2520b.mp3"), "a%20b.mp3");
        assert_eq!(display_name("C:\\Music\\a%20b.mp3"), "a%20b.mp3");
        assert_eq!(display_name("http://localhost/a%ZZ.mp3"), "a%ZZ.mp3");
        assert!(is_audio_source("http://localhost/track.mp3?download=1"));
        assert!(artwork_path("file://other-host/share/art.jpg").is_none());
        assert!(artwork_path("file:///%2Fother-host/share/art.jpg").is_none());
        assert!(artwork_path("file:///%5Cother-host/share/art.jpg").is_none());
        assert!(artwork_path("https://example.com/art.jpg").is_none());
        #[cfg(windows)]
        assert_eq!(
            artwork_path("file:///C:/Album%20Art/cover.jpg"),
            Some("C:/Album Art/cover.jpg".into())
        );
        #[cfg(unix)]
        assert_eq!(
            artwork_path("file:///tmp/Album%20Art/cover.jpg"),
            Some("/tmp/Album Art/cover.jpg".into())
        );
    }

    #[test]
    fn late_artwork_cannot_replace_another_tracks_cover() {
        let (sender, receiver) = crate::latest::channel();
        let mut loader = ArtworkLoader {
            results: Some(receiver),
            ..Default::default()
        };
        let art = image_to_video_frame(image::DynamicImage::new_rgb8(32, 32));
        let _ = sender.replace((
            1,
            "old".into(),
            Some(AudioMetadata {
                thumbnail: Some(art.clone()),
                ..Default::default()
            }),
        ));
        assert!(loader.poll(2).is_none());
        let _ = sender.replace((
            2,
            "new".into(),
            Some(AudioMetadata {
                thumbnail: Some(art),
                ..Default::default()
            }),
        ));
        assert!(loader.poll(2).is_some());
    }

    #[test]
    fn id3_text_respects_encoding_and_bom() {
        assert_eq!(decode_id3_text(&[2, 0, 65, 0, 66]), Some("AB".into()));
        assert_eq!(decode_id3_text(&[1, 0xFE, 0xFF, 0, 65]), Some("A".into()));
        assert_eq!(decode_id3_text(&[1, 0xFF, 0xFE, 65, 0]), Some("A".into()));
        assert_eq!(decode_id3_text(&[0, 0xE9]), Some("é".into()));
    }

    #[test]
    fn test_audio_extensions() {
        assert!(is_audio_file(Path::new("track.mp3")));
        assert!(is_audio_file(Path::new("song.flac")));
        assert!(is_audio_file(Path::new("audio.m4a")));
        assert!(is_audio_file(Path::new("sound.ogg")));
        assert!(is_audio_file(Path::new("sample.wav")));
        assert!(is_audio_file(Path::new("tune.opus")));
        assert!(!is_audio_file(Path::new("movie.mp4")));
        assert!(!is_audio_file(Path::new("video.mkv")));
    }

    #[test]
    fn test_id3v2_tag_parsing() {
        // Construct minimal valid ID3v2.3 buffer with TIT2 = "Test Track"
        let mut buf = Vec::new();
        buf.extend_from_slice(b"ID3\x03\x00\x00"); // ID3v2.3
                                                   // Tag size: 80 bytes synchsafe: 00 00 00 50
        buf.extend_from_slice(&[0, 0, 0, 80]);
        // Frame: TIT2, size: 11 bytes, flags: 00 00, enc: 0, "Test Track"
        buf.extend_from_slice(b"TIT2");
        buf.extend_from_slice(&[0, 0, 0, 11, 0, 0, 0]);
        buf.extend_from_slice(b"Test Track");
        // Frame: TPE1, size: 12 bytes, flags: 00 00, enc: 0, "Test Artist"
        buf.extend_from_slice(b"TPE1");
        buf.extend_from_slice(&[0, 0, 0, 12, 0, 0, 0]);
        buf.extend_from_slice(b"Test Artist");

        let mut meta = AudioMetadata::default();
        parse_id3v2(&buf, &mut meta);
        assert_eq!(meta.title.as_deref(), Some("Test Track"));
        assert_eq!(meta.artist.as_deref(), Some("Test Artist"));
    }
}
