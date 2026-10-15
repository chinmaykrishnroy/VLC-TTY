//! Audio metadata and album artwork extraction.
//! Supports ID3v2 (MP3/WAV/AIFF), FLAC (VORBIS_COMMENT + PICTURE), MP4/M4A/AAC (covr atom),
//! OGG/OPUS, directory cover art fallbacks (cover.jpg/png, folder.jpg/png), and universal image scan.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};
use crate::vlc::VideoFrame;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "wav", "m4a", "aac", "opus", "wma", "ape",
    "alac", "aiff", "aif", "mka", "mid", "midi", "ac3", "dts", "ra",
    "m4b", "m4p", "oga",
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

/// Convert an image::DynamicImage into an owned VideoFrame for TUI rendering
pub fn image_to_video_frame(img: image::DynamicImage) -> Arc<VideoFrame> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    for (src, dst) in rgba.chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
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
    let Ok(mut file) = File::open(path) else { return meta; };

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
        if buf[i] == 0xFF && buf[i + 1] == 0xD8 && buf[i + 2] == 0xFF {
            if let Ok(img) = image::load_from_memory(&buf[i..]) {
                if img.width() >= 32 && img.height() >= 32 {
                    return Some(image_to_video_frame(img));
                }
            }
        } else if buf[i..].starts_with(b"\x89PNG\r\n\x1a\n") {
            if let Ok(img) = image::load_from_memory(&buf[i..]) {
                if img.width() >= 32 && img.height() >= 32 {
                    return Some(image_to_video_frame(img));
                }
            }
        }
    }
    None
}

fn find_folder_artwork(folder: &Path, media_path: &Path) -> Option<Arc<VideoFrame>> {
    let stem = media_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
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
    if data.len() < 10 { return; }
    let version = data[3];
    let tag_size = synchsafe_u32(&data[6..10]) as usize;
    let end = (10 + tag_size).min(data.len());
    let mut offset = 10;

    while offset + 10 < end {
        let (id, frame_size, header_len) = if version == 2 {
            // ID3v2.2 uses 3-byte ID and 3-byte size
            if offset + 6 > end { break; }
            let id = std::str::from_utf8(&data[offset..offset + 3]).unwrap_or("");
            let sz = ((data[offset + 3] as usize) << 16) | ((data[offset + 4] as usize) << 8) | (data[offset + 5] as usize);
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

        if frame_size == 0 || offset + header_len + frame_size > end { break; }
        let payload = &data[offset + header_len..offset + header_len + frame_size];

        match id {
            "TIT2" | "TT2" => if meta.title.is_none() { meta.title = decode_id3_text(payload); },
            "TPE1" | "TP1" => if meta.artist.is_none() { meta.artist = decode_id3_text(payload); },
            "TALB" | "TAL" => if meta.album.is_none() { meta.album = decode_id3_text(payload); },
            "APIC" | "PIC" => {
                if meta.thumbnail.is_none() {
                    if let Some(art) = parse_id3_apic(payload, version) {
                        meta.thumbnail = Some(art);
                    }
                }
            }
            _ => {}
        }
        offset += header_len + frame_size;
    }
}

fn parse_id3_apic(payload: &[u8], version: u8) -> Option<Arc<VideoFrame>> {
    if payload.len() < 5 { return None; }
    let mut idx = 1; // skip encoding byte
    if version == 2 {
        idx += 3; // 3-byte image format (e.g. "JPG", "PNG")
        idx += 1; // picture type
        while idx < payload.len() && payload[idx] != 0 { idx += 1; }
        idx += 1; // skip null
    } else {
        // MIME type is null-terminated ASCII
        while idx < payload.len() && payload[idx] != 0 { idx += 1; }
        idx += 1; // skip null
        if idx >= payload.len() { return None; }
        idx += 1; // skip picture type
        // Skip description (null terminated, either single or double 0)
        while idx < payload.len() && payload[idx] != 0 { idx += 1; }
        if idx < payload.len() && payload[idx] == 0 { idx += 1; }
        if idx < payload.len() && payload[idx] == 0 { idx += 1; }
    }

    if idx < payload.len() {
        let img_bytes = &payload[idx..];
        if let Ok(img) = image::load_from_memory(img_bytes) {
            return Some(image_to_video_frame(img));
        }
    }
    // Fallback: look for JPEG/PNG header in the frame payload directly
    scan_magic_image(payload)
}

fn decode_id3_text(payload: &[u8]) -> Option<String> {
    if payload.is_empty() { return None; }
    let enc = payload[0];
    let body = &payload[1..];
    match enc {
        0 => String::from_utf8(body.iter().filter(|&&b| b != 0).copied().collect()).ok(),
        1 | 2 => {
            // UTF-16 with BOM or BE
            let u16s: Vec<u16> = body.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16(&u16s).ok().map(|s| s.trim_matches('\0').to_string())
        }
        3 => String::from_utf8(body.to_vec()).ok().map(|s| s.trim_matches('\0').to_string()),
        _ => None,
    }.filter(|s| !s.trim().is_empty())
}

fn synchsafe_u32(b: &[u8]) -> u32 {
    ((b[0] as u32 & 0x7F) << 21) | ((b[1] as u32 & 0x7F) << 14) | ((b[2] as u32 & 0x7F) << 7) | (b[3] as u32 & 0x7F)
}

fn u32_be(b: &[u8]) -> u32 {
    ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32)
}

// ──────────────────────────────────────────────────────────────────────
// FLAC Parser (VORBIS_COMMENT and PICTURE metadata blocks)
// ──────────────────────────────────────────────────────────────────────
fn parse_flac(data: &[u8], meta: &mut AudioMetadata) {
    if data.len() < 4 || &data[0..4] != b"fLaC" { return; }
    let mut offset = 4;
    while offset + 4 <= data.len() {
        let header = data[offset];
        let is_last = (header & 0x80) != 0;
        let block_type = header & 0x7F;
        let length = ((data[offset + 1] as usize) << 16) | ((data[offset + 2] as usize) << 8) | (data[offset + 3] as usize);
        offset += 4;
        if offset + length > data.len() { break; }
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
        if is_last { break; }
    }
}

fn parse_flac_picture_block(data: &[u8]) -> Option<Arc<VideoFrame>> {
    if data.len() < 32 { return None; }
    let mime_len = u32_be(&data[4..8]) as usize;
    let mut pos = 8 + mime_len;
    if pos + 4 > data.len() { return None; }
    let desc_len = u32_be(&data[pos..pos + 4]) as usize;
    pos += 4 + desc_len;
    pos += 16; // width (4), height (4), depth (4), colors (4)
    if pos + 4 > data.len() { return None; }
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
    if block.len() < 8 { return; }
    let vendor_len = u32_be(&[block[3], block[2], block[1], block[0]]) as usize;
    let mut pos = 4 + vendor_len;
    if pos + 4 > block.len() { return; }
    let count = u32_be(&[block[pos + 3], block[pos + 2], block[pos + 1], block[pos]]) as usize;
    pos += 4;

    for _ in 0..count.min(200) {
        if pos + 4 > block.len() { break; }
        let len = u32_be(&[block[pos + 3], block[pos + 2], block[pos + 1], block[pos]]) as usize;
        pos += 4;
        if pos + len > block.len() { break; }
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
    let tags = [
        (b"\xa9nam", 1),
        (b"\xa9ART", 2),
        (b"\xa9alb", 3),
    ];
    for (tag, kind) in tags {
        if let Some(pos) = data.windows(4).position(|w| w == tag) {
            let slice = &data[pos..data.len().min(pos + 512)];
            if let Some(dpos) = slice.windows(4).position(|w| w == b"data") {
                let text_start = dpos + 12;
                if text_start < slice.len() {
                    let end = slice[text_start..].iter().position(|&b| b == 0).unwrap_or(slice.len() - text_start);
                    if let Ok(text) = std::str::from_utf8(&slice[text_start..text_start + end]) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            match kind {
                                1 if meta.title.is_none() => meta.title = Some(trimmed.to_string()),
                                2 if meta.artist.is_none() => meta.artist = Some(trimmed.to_string()),
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
