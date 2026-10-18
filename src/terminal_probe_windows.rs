//! Bounded native-console probing. Preserve original input records so shortcuts,
//! modifiers, mouse events and resizes still reach Crossterm after the query.
use ratatui_image::picker::cap_parser::{Parser, QueryStdioOptions};
use std::{
    io::Write,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, PeekConsoleInputW, ReadConsoleInputW, WriteConsoleInputW,
    INPUT_RECORD, KEY_EVENT, STD_INPUT_HANDLE,
};

enum Sequence {
    Pending,
    Reply,
    Input,
}

fn classify(bytes: &[u8]) -> Sequence {
    const KITTY: &[u8] = b"\x1b_Gi=31;";
    if bytes == b"\x1b" || KITTY.starts_with(bytes) {
        return Sequence::Pending;
    }
    if bytes.starts_with(KITTY) {
        return if bytes.ends_with(b"\x1b\\") {
            Sequence::Reply
        } else {
            Sequence::Pending
        };
    }
    if bytes == b"\x1b[" {
        return Sequence::Pending;
    }
    if let Some(body) = bytes.strip_prefix(b"\x1b[") {
        let (prefix, end) = match body.first() {
            Some(b'?') => (b"?".as_slice(), b'c'),
            Some(b'6') => (b"6;".as_slice(), b't'),
            Some(b'0') => {
                return if body == b"0n" {
                    Sequence::Reply
                } else if body == b"0" {
                    Sequence::Pending
                } else {
                    Sequence::Input
                }
            }
            _ => return Sequence::Input,
        };
        if prefix.starts_with(body) {
            return Sequence::Pending;
        }
        if let Some(params) = body.strip_prefix(prefix) {
            let payload = params.strip_suffix(&[end]).unwrap_or(params);
            if payload.iter().all(|b| b.is_ascii_digit() || *b == b';') {
                return if params.ends_with(&[end]) {
                    Sequence::Reply
                } else {
                    Sequence::Pending
                };
            }
        }
    }
    Sequence::Input
}

pub(super) fn query_bytes() -> Vec<u8> {
    // Raw mode is already active and no Crossterm event reader has started.
    // Peek before each read: no blocked reader or altered console mode survives
    // the deadline, including in consoles which never answer these queries.
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut mode = 0;
    if unsafe { GetConsoleMode(input, &mut mode) } == 0 {
        return Vec::new();
    }
    let query = Parser::query(false, QueryStdioOptions::default());
    if std::io::stdout()
        .write_all(query.as_bytes())
        .and_then(|_| std::io::stdout().flush())
        .is_err()
    {
        return Vec::new();
    }
    let deadline = Instant::now() + Duration::from_millis(750);
    let mut replies = Vec::new();
    let mut records = Vec::new();
    let mut pending = Vec::new();
    let mut sequence = Vec::new();
    let mut consumed = 0;
    while Instant::now() < deadline && consumed < 8192 {
        let mut record: INPUT_RECORD = unsafe { std::mem::zeroed() };
        let mut count = 0;
        if unsafe { PeekConsoleInputW(input, &mut record, 1, &mut count) } == 0 {
            break;
        }
        if count == 0 {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut count) } == 0 || count == 0 {
            break;
        }
        consumed += 1;
        let index = records.len();
        records.push((record, true));
        let byte = if u32::from(record.EventType) == KEY_EVENT {
            let key = unsafe { record.Event.KeyEvent };
            let ch = unsafe { key.uChar.UnicodeChar };
            (key.bKeyDown != 0 && key.wRepeatCount == 1 && ch > 0 && ch < 128).then_some(ch as u8)
        } else {
            None
        };
        let Some(byte) = byte else {
            continue;
        };
        if sequence.is_empty() && byte != 0x1b {
            continue;
        }
        // A separate Escape key must not swallow the following reply.
        if byte == 0x1b && !sequence.is_empty() && !sequence.starts_with(b"\x1b_G") {
            sequence.clear();
            pending.clear();
        }
        pending.push(index);
        sequence.push(byte);
        match classify(&sequence) {
            Sequence::Pending => {}
            Sequence::Reply => {
                replies.append(&mut sequence);
                for index in &pending {
                    records[*index].1 = false;
                }
                pending.clear();
                // ConPTY may answer DSR itself before the outer terminal's
                // forwarded DA/cell-size replies arrive. Keep the bounded
                // window open rather than treating DSR as an ordering fence.
            }
            Sequence::Input => {
                pending.clear();
                sequence.clear();
            }
        }
    }
    let preserved: Vec<_> = records
        .into_iter()
        .filter_map(|(record, keep)| keep.then_some(record))
        .collect();
    if !preserved.is_empty() {
        let mut written = 0;
        // Reinsert native events, rather than reconstructing keys from text.
        unsafe {
            WriteConsoleInputW(
                input,
                preserved.as_ptr(),
                preserved.len() as u32,
                &mut written,
            )
        };
    }
    replies
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_probe_replies_are_consumed() {
        for reply in [
            b"\x1b[?62;4c".as_slice(),
            b"\x1b[?1;2c",
            b"\x1b[6;20;10t",
            b"\x1b[0n",
            b"\x1b_Gi=31;OK\x1b\\",
        ] {
            for n in 1..reply.len() {
                assert!(
                    matches!(classify(&reply[..n]), Sequence::Pending),
                    "{reply:?} prefix {n}"
                );
            }
            assert!(matches!(classify(reply), Sequence::Reply));
        }
        for key in [
            b"q".as_slice(),
            b"\x1b[A",
            b"\x1b[15~",
            b"\x1bx",
            b"\x1b[200~",
            b"\x1b[6~",
        ] {
            assert!(matches!(classify(key), Sequence::Input), "{key:?}");
        }
    }
}
