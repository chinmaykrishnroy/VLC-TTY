//! SSH remains responsible for terminal input, resize handling and host-key policy.
#[cfg(feature = "audio-client")]
use anyhow::Context;
use anyhow::{bail, Result};
#[cfg(feature = "audio-client")]
use std::{
    io::Write,
    process::{Command, Stdio},
    time::Duration,
};

pub fn dispatch() -> Result<bool> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return Ok(false);
    };
    if command != "connect" && command != "audio-receive" && command != "stream" {
        return Ok(false);
    }
    let args: Vec<_> = args.collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        if command == "stream" {
            println!("Usage: vlc-tty stream USER@HOST REMOTE_FILE [LOCAL PLAYER OPTIONS]\n\nStreams the original compressed file over SSH into local LibVLC.\nLocal speakers, volume, pause and byte-range seeking use normal local playback.\nNeeds ssh locally and Python 3 on the Linux/macOS server; no remote player required.\nSingle regular files only; directories and remote playlists are not yet supported.\nUse connect when the server should decode media and render the terminal UI.");
        } else if command == "connect" {
            println!("Usage: vlc-tty connect USER@HOST MEDIA [PLAYER OPTIONS]\n\nRuns the installed remote vlc-tty through OpenSSH with a private audio tunnel.\nNeeds matching audio-capable VLC-TTY on the server, local speakers, and ssh on PATH.\nSSH config aliases and saved keys are supported. Host-key checks use OpenSSH defaults.\nUse --remote-command PATH after MEDIA for a custom server executable.\nUse --silent for video only, or --audio-output local for server speakers.\nRemote audio is experimental; measured client lip-sync is not yet validated.");
        } else {
            println!("Usage: vlc-tty audio-receive --listen 127.0.0.1:47650 --token-file PATH\n\nReceives one authenticated S16LE 48kHz stereo session on loopback.\nPATH must contain a 64-character hexadecimal session key shared privately with\n--audio-token-file on the sender. Prefer connect for automatic key/tunnel cleanup.");
        }
        return Ok(true);
    }
    if command == "stream" {
        crate::remote_file::stream(&args)?;
        return Ok(true);
    }
    #[cfg(not(feature = "audio-client"))]
    bail!("This headless build omits the local receiver. Build with --features audio-client on your client computer.");
    #[cfg(feature = "audio-client")]
    {
        if command == "connect" {
            connect(&args)?;
        } else {
            receive(&args)?;
        }
        Ok(true)
    }
}

#[cfg(feature = "audio-client")]
fn receive(args: &[String]) -> Result<()> {
    let mut listen = "127.0.0.1:47650".to_string();
    let mut file = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = args.next().context("--listen needs an address")?.clone(),
            "--token-file" => file = Some(args.next().context("--token-file needs a path")?),
            _ => bail!("Unknown audio-receive option; see audio-receive --help"),
        }
    }
    let key = crate::audio::transport::token(&std::fs::read_to_string(
        file.context("--token-file is required")?,
    )?)?;
    let receiver =
        crate::audio::receiver::Receiver::start(crate::audio::transport::loopback(&listen)?, key)?;
    eprintln!(
        "Experimental audio receiver listening on {}",
        receiver.address
    );
    let termination = crate::termination::Termination::install()?;
    while !receiver.finished() && !termination.requested() {
        std::thread::sleep(Duration::from_millis(100));
    }
    if termination.requested() {
        drop(receiver);
        Ok(())
    } else {
        receiver.wait()
    }
}

pub(crate) fn quote(value: &str) -> Result<String> {
    if value.contains(['\0', '\n', '\r']) {
        bail!("Remote arguments cannot contain NUL or newlines");
    }
    Ok(format!("'{}'", value.replace('\'', "'\"'\"'")))
}

pub(crate) fn validate_host(host: &str) -> Result<()> {
    if host.starts_with('-')
        || host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@._-:[]".contains(&b))
    {
        bail!("Invalid SSH destination; use a host or configured SSH alias");
    }
    Ok(())
}

#[cfg(feature = "audio-client")]
fn connect(args: &[String]) -> Result<()> {
    use rand::RngCore;
    if args.len() < 2 {
        bail!("Usage: vlc-tty connect USER@HOST MEDIA [PLAYER OPTIONS]");
    }
    let host = &args[0];
    validate_host(host)?;
    if !std::io::IsTerminal::is_terminal(&std::io::stdin())
        || !std::io::IsTerminal::is_terminal(&std::io::stdout())
    {
        bail!("connect needs an interactive local terminal");
    }
    let mut remote_binary = "vlc-tty".to_string();
    let mut options = Vec::new();
    let mut requested = crate::audio::AudioOutput::Remote;
    let mut values = args[2..].iter();
    while let Some(value) = values.next() {
        match value.as_str() {
            "--remote-command" => {
                remote_binary = values
                    .next()
                    .context("--remote-command needs a path")?
                    .clone()
            }
            "--silent" => {
                requested = crate::audio::AudioOutput::Disabled;
                options.push(value.clone());
            }
            "--audio" => {
                requested = crate::audio::AudioOutput::Local;
                options.push(value.clone());
            }
            "--audio-output" => {
                let mode = values.next().context("--audio-output needs a mode")?;
                requested = mode.parse()?;
                options.extend([value.clone(), mode.clone()]);
            }
            _ if value.starts_with("--audio-output=") => {
                requested = value[15..].parse()?;
                options.push(value.clone());
            }
            _ => options.push(value.clone()),
        }
    }
    if requested == crate::audio::AudioOutput::Auto {
        requested = crate::audio::AudioOutput::Remote;
    }
    if matches!(
        requested,
        crate::audio::AudioOutput::Disabled | crate::audio::AudioOutput::Local
    ) {
        let mut player = vec![remote_binary, "--ssh".into()];
        player.extend(options);
        player.extend([
            "--audio-output".into(),
            if requested == crate::audio::AudioOutput::Disabled {
                "disabled".into()
            } else {
                "local".into()
            },
            "--".into(),
            args[1].clone(),
        ]);
        let command = player
            .iter()
            .map(|v| quote(v))
            .collect::<Result<Vec<_>>>()?
            .join(" ");
        return ssh_play(host, &command, None);
    }
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let receiver = crate::audio::receiver::Receiver::start("127.0.0.1:0".parse()?, key)?;
    // A separate non-PTY setup request transfers the key on encrypted stdin,
    // never in argv, shell history or terminal echo. Media/audio share the next
    // SSH connection; the temporary key file has mode 0600 and is removed below.
    let mut setup = Command::new("ssh").args(["-o", "ConnectTimeout=15", "--", host,
        "umask 077; p=$(mktemp /tmp/vlc-tty-audio.XXXXXXXXXX) || exit; cat > \"$p\" || exit; printf '%s\\n' \"$p\""])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().context("Cannot start OpenSSH; install ssh and configure access to the server")?;
    {
        let mut input = setup.stdin.take().context("Cannot open SSH setup input")?;
        for byte in key {
            write!(input, "{byte:02x}")?;
        }
        input.write_all(b"\n")?;
    }
    let output = setup.wait_with_output()?;
    if !output.status.success() {
        bail!("SSH audio setup failed: {}", output.status);
    }
    let path = String::from_utf8(output.stdout)?.trim().to_string();
    if !path.starts_with("/tmp/vlc-tty-audio.")
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/.-".contains(&b))
    {
        bail!("SSH setup did not return a valid private session file");
    }
    let session = SessionFile {
        host: host.clone(),
        path,
    };
    let port = 49152 + (rand::random::<u16>() % 16383);
    let endpoint = format!("127.0.0.1:{port}");
    let forward = format!("127.0.0.1:{port}:127.0.0.1:{}", receiver.address.port());
    let mut player = vec![remote_binary, "--ssh".into()];
    player.extend(options);
    // These go last so caller options cannot replace the authenticated endpoint.
    player.extend([
        "--audio-output".into(),
        "remote".into(),
        "--audio-endpoint".into(),
        endpoint,
        "--audio-token-file".into(),
        session.path.clone(),
        "--".into(),
        args[1].clone(),
    ]);
    let player = player
        .iter()
        .map(|value| quote(value))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    let cleanup = format!("rm -f -- {}", quote(&session.path)?);
    let remote = format!("trap {} EXIT HUP INT TERM; {player}", quote(&cleanup)?);
    eprintln!("Connecting to {host} with experimental local audio. Remote VLC-TTY must support --audio-output remote.");
    let result = ssh_play(host, &remote, Some(&forward));
    drop(receiver);
    drop(session);
    result
}

#[cfg(feature = "audio-client")]
fn ssh_play(host: &str, remote: &str, forward: Option<&str>) -> Result<()> {
    let termination = crate::termination::Termination::install()?;
    let mut command = Command::new("ssh");
    command.args([
        "-tt",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
    ]);
    if let Some(forward) = forward {
        command.args(["-R", forward]);
    }
    let mut child = command.args(["--", host, remote]).spawn()?;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if termination.requested() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("SSH playback interrupted or local audio receiver disconnected");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        bail!("SSH playback exited with {status}; verify the remote version and tunnel port availability");
    }
    Ok(())
}

#[cfg(feature = "audio-client")]
struct SessionFile {
    host: String,
    path: String,
}
#[cfg(feature = "audio-client")]
impl Drop for SessionFile {
    fn drop(&mut self) {
        // Also handles failure before the remote shell installs its trap. Batch
        // mode prevents a cleanup request from opening a new authentication UI.
        if let Ok(path) = quote(&self.path) {
            let _ = Command::new("ssh")
                .args([
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ConnectTimeout=3",
                    "--",
                    &self.host,
                    &format!("rm -f -- {path}"),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_shell_arguments_cannot_execute_media_names() {
        assert_eq!(
            quote("movie's $(touch BAD).mkv").unwrap(),
            "'movie'\"'\"'s $(touch BAD).mkv'"
        );
        assert!(quote("movie\ncommand").is_err());
    }
}
