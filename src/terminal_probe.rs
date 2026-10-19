//! Synchronous startup probing: no reader can outlive the timeout and steal keys
//! or restore cooked mode underneath the TUI. Never call the image crate's
//! thread-based stdio query on a terminal that may not answer.
use ratatui::crossterm::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    terminal,
};
use ratatui_image::{
    picker::{
        cap_parser::{Parser, QueryStdioOptions, Response},
        Picker, ProtocolType,
    },
    FontSize,
};
use std::collections::VecDeque;
#[cfg(unix)]
use std::time::{Duration, Instant};

pub struct Detection {
    pub picker: Picker,
    pub events: VecDeque<Event>,
    pub kitty_compression: bool,
}

pub(super) fn query_options() -> QueryStdioOptions {
    QueryStdioOptions {
        kitty_compression: true,
        ..QueryStdioOptions::default()
    }
}

pub fn detect(query: bool) -> Detection {
    let fallback = terminal::window_size()
        .ok()
        .and_then(|s| {
            if s.columns > 0 && s.rows > 0 && s.width >= s.columns && s.height >= s.rows {
                Some(FontSize::new(s.width / s.columns, s.height / s.rows))
            } else {
                None
            }
        })
        .unwrap_or(FontSize::new(10, 20));
    let env = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let term = std::env::var("TERM").unwrap_or_default();
    let protocol = if !query {
        ProtocolType::Halfblocks
    } else if env == "iTerm.app" || env == "WezTerm" {
        ProtocolType::Iterm2
    } else if term.contains("kitty") {
        ProtocolType::Kitty
    } else {
        ProtocolType::Halfblocks
    };
    parse_responses(
        fallback,
        protocol,
        if query { query_bytes() } else { Vec::new() },
    )
}

fn parse_responses(
    mut font: FontSize,
    mut protocol: ProtocolType,
    responses: Vec<u8>,
) -> Detection {
    let mut events = VecDeque::new();
    let mut kitty_compression = false;
    {
        let mut parser = Parser::new();
        let mut sequence = Vec::new();
        let mut text = Vec::new();
        for byte in responses {
            if sequence.is_empty() && byte != 0x1b {
                text.push(byte);
                continue;
            }
            sequence.push(byte);
            let found = parser.push(char::from(byte));
            if !found.is_empty() {
                for response in found {
                    match response {
                        Response::Kitty => protocol = ProtocolType::Kitty,
                        Response::KittyCompression => kitty_compression = true,
                        Response::Sixel if protocol != ProtocolType::Kitty => {
                            protocol = ProtocolType::Sixel
                        }
                        Response::CellSize(Some((w, h))) if w > 0 && h > 0 => {
                            font = FontSize::new(w, h)
                        }
                        _ => {}
                    }
                }
                sequence.clear();
            } else if sequence.len() >= 3 && sequence[1] == b'[' && (0x40..=0x7e).contains(&byte) {
                let code = match sequence.as_slice() {
                    b"\x1b[A" => Some(KeyCode::Up),
                    b"\x1b[B" => Some(KeyCode::Down),
                    b"\x1b[C" => Some(KeyCode::Right),
                    b"\x1b[D" => Some(KeyCode::Left),
                    _ => None,
                };
                if let Some(code) = code {
                    events.push_back(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
                }
                sequence.clear();
            } else if sequence.len() > 4096 {
                sequence.clear();
            }
        }
        if sequence == b"\x1b" {
            events.push_back(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        }
        for ch in String::from_utf8_lossy(&text).chars() {
            let (code, modifier) = match ch {
                '\r' | '\n' => (KeyCode::Enter, KeyModifiers::NONE),
                '\t' => (KeyCode::Tab, KeyModifiers::NONE),
                '\x03' => (KeyCode::Char('c'), KeyModifiers::CONTROL),
                _ => (KeyCode::Char(ch), KeyModifiers::NONE),
            };
            events.push_back(Event::Key(KeyEvent::new(code, modifier)));
        }
    }
    // Public construction API without any stdio probing. This constructor is
    // deprecated in favor of the threaded query that can retain stdin after timeout, not removed.
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(font);
    picker.set_protocol_type(protocol);
    Detection {
        picker,
        events,
        kitty_compression,
    }
}

#[cfg(unix)]
fn query_bytes() -> Vec<u8> {
    use std::io::Write;
    let is_tmux = std::env::var_os("TMUX").is_some();
    let query = Parser::query(is_tmux, query_options());
    if std::io::stdout()
        .write_all(query.as_bytes())
        .and_then(|_| std::io::stdout().flush())
        .is_err()
    {
        return Vec::new();
    }
    let deadline = Instant::now() + Duration::from_millis(750);
    let mut bytes = Vec::new();
    while Instant::now() < deadline && bytes.len() < 8192 {
        let mut fd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .max(1) as i32;
        let ready = unsafe { libc::poll(&mut fd, 1, remaining) };
        if ready <= 0 || fd.revents & libc::POLLIN == 0 {
            break;
        }
        let mut buffer = [0u8; 256];
        let length = unsafe { libc::read(0, buffer.as_mut_ptr().cast(), buffer.len()) };
        if length <= 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..length as usize]);
        if bytes.windows(4).any(|part| part == b"\x1b[0n") {
            break;
        }
    }
    bytes
}
#[cfg(windows)]
#[path = "terminal_probe_windows.rs"]
mod windows;
#[cfg(windows)]
fn query_bytes() -> Vec<u8> {
    windows::query_bytes()
}
#[cfg(not(any(unix, windows)))]
fn query_bytes() -> Vec<u8> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_query_and_success_survive_the_synchronous_probe() {
        let query = Parser::query(false, query_options());
        assert!(
            query.contains("i=32") && query.contains("o=z"),
            "compression must actually be probed"
        );
        let result = parse_responses(
            FontSize::new(10, 20),
            ProtocolType::Halfblocks,
            b"\x1b_Gi=31;OK\x1b\\\x1b_Gi=32;OK\x1b\\".to_vec(),
        );
        assert_eq!(result.picker.protocol_type(), ProtocolType::Kitty);
        assert!(
            result.kitty_compression,
            "confirmed compression was discarded"
        );
    }

    #[test]
    fn absent_or_rejected_compression_never_enables_it() {
        for replies in [
            b"".as_slice(),
            b"\x1b_Gi=31;OK\x1b\\",
            b"\x1b_Gi=31;OK\x1b\\\x1b_Gi=32;EINVAL\x1b\\",
        ] {
            assert!(
                !parse_responses(FontSize::new(10, 20), ProtocolType::Kitty, replies.to_vec())
                    .kitty_compression
            );
        }
    }
}
