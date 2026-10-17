//! UTF-8 M3U queues. Resolve paths relative to the playlist, never the process CWD.
use anyhow::{bail, Context, Result};
use std::{fs, io::Write, path::Path};

pub fn load(path: &Path) -> Result<Vec<String>> {
    if fs::metadata(path)?.len() > 4 * 1024 * 1024 {
        bail!("Playlist exceeds the 4 MiB limit");
    }
    let text = fs::read_to_string(path).context("Playlist must be UTF-8 M3U/M3U8")?;
    let parent = path
        .canonicalize()?
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut sources = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains('\0') {
            bail!("Playlist contains a NUL byte");
        }
        let source = if line.contains("://") || Path::new(line).is_absolute() {
            line.to_owned()
        } else {
            parent.join(line).to_string_lossy().into_owned()
        };
        let source = if source.contains("://") {
            source
        } else {
            Path::new(&source)
                .canonicalize()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or(source)
        };
        sources.push(source);
        if sources.len() > 10_000 {
            bail!("Playlist exceeds 10,000 entries");
        }
    }
    if sources.is_empty() {
        bail!("Playlist contains no media entries");
    }
    Ok(sources)
}

pub fn save(path: &Path, sources: &[String]) -> Result<()> {
    if sources.is_empty() {
        bail!("There are no queue entries to export");
    }
    if sources
        .iter()
        .any(|source| source.contains(['\r', '\n', '\0']) || source.starts_with('#'))
    {
        bail!("An entry cannot be represented safely in an M3U playlist");
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "#EXTM3U")?;
    for source in sources {
        // Export absolute local paths so a saved queue survives moving the
        // playlist to another directory. Preserve URLs including their fragments.
        let source = if source.contains("://") || Path::new(source).is_absolute() {
            source.clone()
        } else {
            std::env::current_dir()?
                .join(source)
                .to_string_lossy()
                .into_owned()
        };
        writeln!(file, "{source}")?;
    }
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|error| error.error)
        .context("Could not save playlist")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_resolves_local_paths_and_preserves_url_fragments() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("queue.m3u8");
        fs::write(
            &file,
            "\u{feff}#EXTM3U\n#EXTINF:-1,Example\nmovie.mp4\nhttps://example.com/a#chapter\n",
        )
        .unwrap();
        let sources = load(&file).unwrap();
        assert!(Path::new(&sources[0]).is_absolute());
        assert!(sources[0].ends_with("movie.mp4"));
        assert_eq!(sources[1], "https://example.com/a#chapter");
    }
    #[test]
    fn export_round_trips_and_rejects_line_injection() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("queue.m3u8");
        let sources = vec!["https://example.com/clip?a=b#chapter".into()];
        save(&file, &sources).unwrap();
        assert_eq!(load(&file).unwrap(), sources);
        assert!(save(&file, &["one.mp4\nhttps://unexpected.test".into()]).is_err());
        assert_eq!(
            load(&file).unwrap(),
            sources,
            "Rejected export must preserve existing playlist"
        );
    }
}
