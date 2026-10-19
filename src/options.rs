//! Resolve the SSH profile once at startup; explicit flags override profile defaults.
use crate::audio::AudioOutput;
use crate::graphics::GraphicsChoice;
use crate::queue::RepeatMode;
use anyhow::{bail, Context, Result};

#[derive(Debug)]
pub struct Options {
    pub media: Option<String>,
    pub subtitle: Option<String>,
    pub playlist: Option<String>,
    pub export_playlist: Option<String>,
    pub metrics: Option<String>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub volume: i32,
    pub rate: f32,
    pub graphics: GraphicsChoice,
    pub kitty_compression: bool,
    pub fps: u32,
    pub decode_height: u32,
    pub native_window: bool,
    pub audio_output: AudioOutput,
    pub audio_endpoint: Option<String>,
    pub audio_token_file: Option<String>,
    pub direct_ssh_audio_warning: bool,
    pub ssh: bool,
    pub doctor: bool,
    pub smoke_test: bool,
    pub help: bool,
    pub version: bool,
}

impl Options {
    pub fn parse(
        args: impl IntoIterator<Item = String>,
        remote: bool,
        env_decode: Option<&str>,
    ) -> Result<Self> {
        let mut out = Self {
            media: None,
            subtitle: None,
            playlist: None,
            export_playlist: None,
            metrics: None,
            repeat: RepeatMode::Off,
            shuffle: false,
            volume: 100,
            rate: 1.0,
            graphics: GraphicsChoice::Auto,
            kitty_compression: true,
            fps: 60,
            decode_height: 1080,
            native_window: false,
            audio_output: AudioOutput::Auto,
            audio_endpoint: None,
            audio_token_file: None,
            direct_ssh_audio_warning: false,
            ssh: remote,
            doctor: false,
            smoke_test: false,
            help: false,
            version: false,
        };
        let mut args = args.into_iter();
        let mut fps = None;
        let mut decode = None;
        let mut audio = None;
        let mut positional = false;
        while let Some(arg) = args.next() {
            if positional {
                if out.media.replace(arg).is_some() {
                    bail!("Only one media path, folder or URL may be specified");
                }
                continue;
            }
            let (name, inline) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
            if inline.is_some()
                && matches!(
                    name,
                    "--" | "-h"
                        | "--help"
                        | "-V"
                        | "--version"
                        | "--doctor"
                        | "--smoke-test"
                        | "--ssh"
                        | "--silent"
                        | "--audio"
                        | "--native-window"
                        | "--shuffle"
                        | "--no-kitty-compression"
                )
            {
                bail!("{name} does not take an inline value");
            }
            let mut value = || {
                inline
                    .map(str::to_owned)
                    .or_else(|| args.next())
                    .with_context(|| format!("{name} needs a value"))
            };
            match name {
                "--" => positional = true,
                "-h" | "--help" => out.help = true,
                "-V" | "--version" => out.version = true,
                "--doctor" => out.doctor = true,
                "--smoke-test" => out.smoke_test = true,
                "--ssh" => out.ssh = true,
                "--silent" => audio = Some(AudioOutput::Disabled),
                "--audio" => audio = Some(AudioOutput::Local),
                "--audio-output" => audio = Some(value()?.parse()?),
                "--audio-endpoint" => out.audio_endpoint = Some(value()?),
                "--audio-token-file" => out.audio_token_file = Some(value()?),
                "--native-window" => out.native_window = true,
                "--shuffle" => out.shuffle = true,
                "--metrics" => out.metrics = Some(value()?),
                "--playlist" => out.playlist = Some(value()?),
                "--export-playlist" => out.export_playlist = Some(value()?),
                "--repeat" => {
                    out.repeat = match value()?.as_str() {
                        "off" => RepeatMode::Off,
                        "all" => RepeatMode::All,
                        "one" => RepeatMode::One,
                        _ => bail!("--repeat must be off, all or one"),
                    }
                }
                "--volume" => {
                    out.volume = value()?.parse().context("--volume must be an integer")?
                }
                "--rate" => out.rate = value()?.parse().context("--rate must be a number")?,
                "--graphics" => out.graphics = value()?.parse().map_err(anyhow::Error::msg)?,
                "--no-kitty-compression" => out.kitty_compression = false,
                "--subtitle" => out.subtitle = Some(value()?),
                "--fps" => {
                    fps = Some(
                        value()?
                            .parse::<u32>()
                            .context("--fps must be an integer")?,
                    )
                }
                "--decode-height" => {
                    decode = Some(
                        value()?
                            .parse::<u32>()
                            .context("--decode-height must be an integer")?,
                    )
                }
                _ if arg.starts_with('-') => bail!("Unknown option: {arg} (see --help)"),
                _ => {
                    if out.media.replace(arg).is_some() {
                        bail!("Only one media path, folder or URL may be specified");
                    }
                }
            }
        }
        if out.help || out.version {
            return Ok(out);
        }
        if out.media.is_some() && out.playlist.is_some() {
            bail!("Specify either media or --playlist, not both");
        }
        if out.metrics.is_some()
            && (out.doctor || out.smoke_test || out.export_playlist.is_some() || out.native_window)
        {
            bail!(
                "--metrics profiles terminal playback; omit diagnostic/export/native-window modes"
            );
        }
        if !(0..=125).contains(&out.volume) {
            bail!("--volume must be between 0 and 125");
        }
        if !out.rate.is_finite() || !(0.25..=4.0).contains(&out.rate) {
            bail!("--rate must be between 0.25 and 4");
        }
        if out.export_playlist.is_some() && (out.doctor || out.smoke_test) {
            bail!("--export-playlist cannot be combined with diagnostic modes");
        }
        out.fps = fps.unwrap_or(if out.ssh { 15 } else { 60 });
        out.decode_height = match decode {
            Some(height) => height,
            None => match env_decode {
                Some(text) => text
                    .parse::<u32>()
                    .context("TERMVLC_DECODE_MAX must be an integer")?,
                None => {
                    if out.ssh {
                        720
                    } else {
                        1080
                    }
                }
            },
        };
        let requested = audio.unwrap_or(AudioOutput::Auto);
        out.direct_ssh_audio_warning = out.ssh && requested == AudioOutput::Auto;
        out.audio_output = if out.smoke_test {
            AudioOutput::Disabled
        } else {
            match requested {
                AudioOutput::Auto => {
                    if out.ssh {
                        AudioOutput::Disabled
                    } else {
                        AudioOutput::Local
                    }
                }
                mode => mode,
            }
        };
        if out.audio_output == AudioOutput::Remote {
            let endpoint = out.audio_endpoint.as_deref().context("Remote audio needs --audio-endpoint and an authenticated receiver; use vlc-tty connect")?;
            crate::audio::transport::loopback(endpoint)?;
            if out.audio_token_file.is_none() {
                bail!("Remote audio needs --audio-token-file; use vlc-tty connect");
            }
        } else if out.audio_endpoint.is_some() || out.audio_token_file.is_some() {
            bail!("Audio endpoint/session key require --audio-output remote");
        }
        if !(1..=120).contains(&out.fps) {
            bail!("--fps must be between 1 and 120");
        }
        if out.decode_height > 4320 {
            bail!("--decode-height must be 0 (uncapped) or at most 4320");
        }
        if out.ssh && out.native_window {
            bail!("--native-window needs a local desktop; omit it for terminal playback over SSH");
        }
        if out.smoke_test && out.media.is_none() {
            bail!("--smoke-test needs a media path or URL");
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_and_version_ignore_invalid_runtime_environment() {
        for flag in ["--help", "-h", "--version", "-V"] {
            assert!(Options::parse([flag.to_owned()], true, Some("bad height")).is_ok());
        }
    }
    fn parse(args: &[&str], remote: bool) -> Result<Options> {
        Options::parse(args.iter().map(|arg| arg.to_string()), remote, None)
    }
    #[test]
    fn ssh_is_silent_and_bounded_by_default() {
        let options = parse(&[], true).unwrap();
        assert!(
            options.audio_output == AudioOutput::Disabled
                && options.ssh
                && options.direct_ssh_audio_warning
        );
        assert_eq!((options.fps, options.decode_height), (15, 720));
    }
    #[test]
    fn explicit_flags_override_profile_in_any_order() {
        for args in [
            vec!["--fps=24", "--decode-height", "0", "--audio", "--ssh"],
            vec!["--ssh", "--fps", "24", "--decode-height=0", "--audio"],
        ] {
            let options = parse(&args, false).unwrap();
            assert_eq!(
                (options.fps, options.decode_height, options.audio_output),
                (24, 0, AudioOutput::Local)
            );
        }
    }
    #[test]
    fn kitty_compression_can_be_disabled_without_changing_renderer_or_ssh_defaults() {
        assert!(parse(&[], false).unwrap().kitty_compression);
        let options = parse(
            &["--ssh", "--graphics", "kitty", "--no-kitty-compression"],
            false,
        )
        .unwrap();
        assert!(!options.kitty_compression);
        assert_eq!(options.graphics, GraphicsChoice::Kitty);
        assert_eq!(options.fps, 15);
        assert!(parse(&["--no-kitty-compression=true"], false).is_err());
    }
    #[test]
    fn audio_modes_require_an_authenticated_loopback_receiver() {
        let options = parse(
            &[
                "--ssh",
                "--audio-output",
                "remote",
                "--audio-endpoint",
                "127.0.0.1:47650",
                "--audio-token-file",
                "session.key",
            ],
            false,
        )
        .unwrap();
        assert_eq!(options.audio_output, AudioOutput::Remote);
        assert!(!options.direct_ssh_audio_warning);
        assert_eq!(parse(&[], false).unwrap().audio_output, AudioOutput::Local);
        assert!(
            !parse(&["--ssh", "--silent"], false)
                .unwrap()
                .direct_ssh_audio_warning
        );
        for args in [
            vec!["--audio-output", "remote"],
            vec![
                "--audio-output",
                "remote",
                "--audio-endpoint",
                "0.0.0.0:47650",
                "--audio-token-file",
                "key",
            ],
            vec!["--audio-output", "invalid"],
            vec!["--audio-endpoint", "127.0.0.1:1"],
        ] {
            assert!(parse(&args, false).is_err());
        }
    }
    #[test]
    fn invalid_options_fail_before_terminal_or_decoder_start() {
        for args in [
            vec!["--fps=0"],
            vec!["--decode-height=99999"],
            vec!["--ssh", "--native-window"],
            vec!["--smoke-test"],
            vec!["--graphics=unknown"],
            vec!["--silent=false"],
            vec!["--audio=false"],
            vec!["one", "two"],
            vec!["--rate=NaN"],
            vec!["--rate=0"],
            vec!["--volume=126"],
            vec!["--shuffle=false"],
            vec!["--repeat=invalid"],
            vec!["--playlist", "queue.m3u", "movie.mp4"],
            vec!["--metrics=run.jsonl", "--doctor"],
            vec!["--metrics=run.jsonl", "--native-window"],
            vec!["--metrics=run.jsonl", "--smoke-test", "movie.mp4"],
        ] {
            assert!(parse(&args, false).is_err(), "{args:?}");
        }
        assert!(Options::parse(Vec::new(), false, Some("invalid")).is_err());
    }
    #[test]
    fn playlist_controls_and_export_parse_without_media_or_runtime() {
        let opts = parse(
            &[
                "--playlist=queue.m3u8",
                "--shuffle",
                "--repeat=all",
                "--volume=80",
                "--rate=1.5",
                "--export-playlist=copy.m3u8",
            ],
            false,
        )
        .unwrap();
        assert!(opts.shuffle);
        assert_eq!(opts.repeat, RepeatMode::All);
        assert_eq!((opts.volume, opts.rate), (80, 1.5));
        assert_eq!(opts.playlist.as_deref(), Some("queue.m3u8"));
    }
    #[test]
    fn paths_starting_with_dash_and_headless_smoke_are_supported() {
        let options = parse(&["--smoke-test", "--", "-movie.mp4"], false).unwrap();
        assert_eq!(options.media.as_deref(), Some("-movie.mp4"));
        assert_eq!(options.audio_output, AudioOutput::Disabled);
    }
}
