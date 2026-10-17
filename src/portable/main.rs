//! A portable file embeds the player and native runtime, extracts privately and
//! runs the player with inherited terminal handles. No installer or system VLC.
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

#[path = "../help.rs"]
mod help;
include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

fn unpack(payload: &[u8], hash: &str, destination: &Path) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(payload));
    if actual != hash {
        bail!("Embedded payload checksum failed; download a fresh executable");
    }
    let decoder = GzDecoder::new(Cursor::new(payload));
    let mut archive = tar::Archive::new(decoder);
    // tar rejects paths that escape the destination. The payload itself is
    // immutable and hashed; no writable cache is reused across invocations.
    archive.unpack(destination).context(
        "Could not extract portable runtime; check free space and temporary-directory permissions",
    )?;
    Ok(())
}

fn command(directory: &Path) -> Result<Command> {
    let executable = directory.join(if cfg!(windows) {
        "app/vlc-tty.exe"
    } else {
        "app/vlc-tty"
    });
    if !executable.is_file() {
        bail!("Portable payload is missing the player executable");
    }
    let mut command = Command::new(executable);
    let runtime = env::var_os("VLC_TTY_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                directory.join("runtime/VLC.app")
            } else if cfg!(windows) {
                directory.join("runtime")
            } else {
                directory.join("runtime/lib")
            }
        });
    command
        .env("VLC_HOME", &runtime)
        .env("VLC_TTY_STRICT_RUNTIME", "1")
        .env_remove("VLC_PLUGIN_PATH");
    if cfg!(target_os = "linux") {
        command.env("VLC_PLUGIN_PATH", runtime.join("vlc/plugins"));
        let existing = env::var_os("LD_LIBRARY_PATH").unwrap_or_default();
        let paths =
            env::join_paths(std::iter::once(runtime.clone()).chain(env::split_paths(&existing)))?;
        // Set before starting a fresh process, not after dlopen has initialized.
        command.env("LD_LIBRARY_PATH", paths);
    }
    if cfg!(windows) {
        let existing = env::var_os("PATH").unwrap_or_default();
        command.env(
            "PATH",
            env::join_paths(std::iter::once(runtime).chain(env::split_paths(&existing)))?,
        );
    }
    Ok(command)
}

fn run() -> Result<u8> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        help::print(true);
        return Ok(0);
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        println!("VLC-TTY {} (portable)", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    if args.len() == 1 && args[0] == "--bundle-info" {
        println!("{BUNDLE_INFO}");
        return Ok(0);
    }
    if PAYLOAD.is_empty() {
        bail!("This development launcher has no runtime. Build a portable file with scripts/build_portable.py");
    }
    if args.first().is_some_and(|arg| arg == "--extract-to") {
        if args.len() != 2 {
            bail!("Usage: vlc-tty --extract-to NEW_DIRECTORY");
        }
        let path = PathBuf::from(&args[1]);
        fs::create_dir(&path).context("Extraction destination must be a new directory")?;
        unpack(PAYLOAD, PAYLOAD_SHA, &path)?;
        println!(
            "Extracted player, runtime, licenses and build information to {}",
            path.display()
        );
        return Ok(0);
    }
    let temporary = tempfile::Builder::new().prefix("vlc-tty-").tempdir()?;
    unpack(PAYLOAD, PAYLOAD_SHA, temporary.path())?;
    let status = command(temporary.path())?.args(args).status().context(
        "Could not start the extracted player. On Linux, TMPDIR must allow executable files",
    )?;
    Ok(status.code().unwrap_or(1).clamp(0, 255) as u8)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("VLC-TTY: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_payload_fails_before_extraction() {
        let temp = tempfile::tempdir().unwrap();
        assert!(unpack(b"corrupt", &"0".repeat(64), temp.path()).is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}
