//! Original compressed bytes over SSH; local LibVLC owns playback and seeking.
use anyhow::{bail, Context, Result};
use std::{
    io::{BufRead, BufReader, IsTerminal, Read},
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const SERVER: &str = include_str!("remote_file_server.py");

struct Tunnel(Child);
impl Drop for Tunnel {
    fn drop(&mut self) {
        // Remote Python exits on EOF. Give it time to close its loopback socket.
        drop(self.0.stdin.take());
        for _ in 0..30 {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct LocalTerminal(bool);
impl LocalTerminal {
    fn enter() -> Result<Self> {
        let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        if interactive {
            // Own the original console mode in this process too. If the child
            // must be killed after SSH loss, its own Drop cannot restore it.
            ratatui::crossterm::terminal::enable_raw_mode()?;
        }
        Ok(Self(interactive))
    }
}
impl Drop for LocalTerminal {
    fn drop(&mut self) {
        if self.0 {
            use ratatui::crossterm::{cursor, event, execute, terminal};
            let _ = terminal::disable_raw_mode();
            let _ = execute!(
                std::io::stdout(),
                terminal::EndSynchronizedUpdate,
                event::DisableMouseCapture,
                event::DisableBracketedPaste,
                cursor::Show,
                terminal::LeaveAlternateScreen
            );
        }
    }
}

struct LocalPlayer(Child);
impl Drop for LocalPlayer {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn encoded_name(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or("media");
    name.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn stream(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        bail!("Usage: vlc-tty stream USER@HOST REMOTE_FILE [LOCAL PLAYER OPTIONS]");
    }
    crate::remote::validate_host(&args[0])?;
    // Reuse the normal option parser, including -- and option-value handling.
    let mut validation = args[2..].to_vec();
    validation.extend(["--".into(), "http://127.0.0.1/remote-file".into()]);
    let options = crate::options::Options::parse(validation, false, None)?;
    if options.playlist.is_some()
        || options.export_playlist.is_some()
        || options.audio_output == crate::audio::AudioOutput::Remote
    {
        bail!("stream accepts one remote file and local playback options; use connect for server rendering");
    }
    let local = TcpListener::bind("127.0.0.1:0")?;
    let port = local.local_addr()?.port();
    let remote_port = 49152 + rand::random::<u16>() % 16383;
    let token = format!("{:032x}", rand::random::<u128>());
    let script = SERVER
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let command = format!(
        "python3 -u -c {} {} {} {}",
        crate::remote::quote(&format!("exec(bytes.fromhex('{script}'))"))?,
        crate::remote::quote(&args[1])?,
        crate::remote::quote(&token)?,
        remote_port
    );
    let forward = format!("127.0.0.1:{port}:127.0.0.1:{remote_port}");
    drop(local);
    let mut tunnel = Tunnel(
        Command::new("ssh")
            .args([
                "-T",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ConnectTimeout=15",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=2",
                "-L",
                &forward,
                "--",
                &args[0],
                &command,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Cannot start OpenSSH")?,
    );
    let stdout = tunnel
        .0
        .stdout
        .take()
        .context("Missing SSH server handshake pipe")?;
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut ready = String::new();
        let result = BufReader::new(stdout).take(1024).read_line(&mut ready);
        let _ = tx.send(result.map(|_| ready));
    });
    let ready = rx.recv_timeout(Duration::from_secs(60));
    if !matches!(ready, Ok(Ok(ref value)) if value.trim() == "READY") {
        drop(tunnel); // Closing/killing SSH also unblocks the handshake reader.
        let _ = reader.join();
        bail!("Remote stream setup failed or timed out. Needs Python 3 and a readable regular file on the server; check SSH errors above.");
    }
    let _ = reader.join();
    let url = format!("http://127.0.0.1:{port}/{token}/{}", encoded_name(&args[1]));
    eprintln!(
        "Streaming original file bytes from {} over SSH; decoding and controls run locally.",
        args[0]
    );
    let termination = crate::termination::Termination::install()?;
    let _terminal = LocalTerminal::enter()?;
    let mut player = LocalPlayer(
        Command::new(std::env::current_exe()?)
            .args(&args[2..])
            .arg("--")
            .arg(url)
            .spawn()
            .context("Cannot start local player")?,
    );
    let status = loop {
        if let Some(status) = player.0.try_wait()? {
            break status;
        }
        if termination.requested() || tunnel.0.try_wait()?.is_some() {
            let _ = player.0.kill();
            let _ = player.0.wait();
            bail!("Remote file stream interrupted or SSH disconnected");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if !status.success() {
        bail!("Local playback failed: {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_filename_is_one_encoded_url_component() {
        assert_eq!(encoded_name("/Music/a b?#é.mp3"), "a%20b%3F%23%C3%A9.mp3");
    }
}
