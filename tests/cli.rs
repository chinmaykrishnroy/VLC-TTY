use std::{fs, process::Command};

fn player() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vlc-tty"));
    command.env("VLC_HOME", "intentionally-missing-runtime");
    command.env("VLC_TTY_STRICT_RUNTIME", "1");
    command
}

#[test]
fn playlist_export_works_without_runtime_or_terminal() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("queue.m3u8");
    let output = directory.path().join("saved queue.m3u8");
    fs::write(
        &source,
        "#EXTM3U\nhttps://example.test/clip#chapter\ntrack name.mp4\n",
    )
    .unwrap();
    let result = player()
        .arg("--playlist")
        .arg(&source)
        .arg("--export-playlist")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let saved = fs::read_to_string(output).unwrap();
    assert!(saved.contains("https://example.test/clip#chapter"));
    assert!(saved.contains(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("track name.mp4")
            .to_string_lossy()
            .to_string()
    ));
}

#[test]
fn missing_media_is_reported_before_terminal_or_backend_startup() {
    let directory = tempfile::tempdir().unwrap();
    let result = player()
        .arg("--ssh")
        .arg(directory.path().join("missing.mp4"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("Media path not found"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn audio_command_help_does_not_need_a_runtime_or_sound_device() {
    for command in ["connect", "audio-receive", "stream"] {
        let result = player().args([command, "--help"]).output().unwrap();
        assert!(result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(&format!("Usage: vlc-tty {command}"))
        );
    }
}
