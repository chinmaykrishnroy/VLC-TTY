//! Recognize a complete unbracketed file drop without discarding ordinary events.
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

pub fn read_batch(
    first: Event,
    mut next: impl FnMut(Duration) -> io::Result<Option<Event>>,
) -> io::Result<Vec<Event>> {
    let mut events = vec![first];
    let deadline = Instant::now() + Duration::from_millis(150);
    while events.len() < 4096 && Instant::now() < deadline {
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(40));
        let Some(event) = next(wait)? else {
            break;
        };
        let is_text = matches!(&event, Event::Key(k) if matches!(k.code, KeyCode::Char(_))
            && !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT));
        events.push(event);
        if !is_text {
            break;
        }
    }
    Ok(events)
}

pub struct FileDrop {
    pub path: String,
    pub consumed: usize,
}

pub fn dropped_path(events: &[Event]) -> Option<FileDrop> {
    let mut text = String::new();
    let mut consumed = 0;
    for event in events {
        let Event::Key(key) = event else {
            break;
        };
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            break;
        }
        if key.kind == KeyEventKind::Release {
            consumed += 1;
            continue;
        }
        match key.code {
            KeyCode::Char(c) => {
                text.push(c);
                consumed += 1;
            }
            _ => break,
        }
    }
    let path = text
        .trim()
        .trim_start_matches('&')
        .trim()
        .trim_matches(['\'', '"']);
    let drive_path = path
        .as_bytes()
        .get(1..3)
        .is_some_and(|part| part == b":\\" || part == b":/");
    if path.len() > 1
        && (Path::new(path).exists() || path.contains("://") || drive_path || path.starts_with('/'))
    {
        Some(FileDrop {
            path: path.to_owned(),
            consumed,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEvent;

    fn key(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    #[test]
    fn shortcuts_and_non_key_events_are_preserved() {
        let mut incoming = vec![key('g'), Event::Resize(100, 40)].into_iter();
        let events = read_batch(key('f'), |_| Ok(incoming.next())).unwrap();
        assert_eq!(events, vec![key('f'), key('g'), Event::Resize(100, 40)]);
        assert!(dropped_path(&events).is_none());
        assert!(dropped_path(&events[..2]).is_none());
    }

    #[test]
    fn complete_file_drop_is_recognized() {
        let events: Vec<_> = "\"samples/sample.mp4\"".chars().map(key).collect();
        assert_eq!(
            dropped_path(&events).map(|drop| drop.path).as_deref(),
            Some("samples/sample.mp4")
        );
    }

    #[test]
    fn file_drop_leaves_following_resize_available_for_dispatch() {
        let mut events: Vec<_> = "\"samples/sample.mp4\"".chars().map(key).collect();
        events.push(Event::Resize(80, 24));
        let drop = dropped_path(&events).unwrap();
        assert_eq!(drop.path, "samples/sample.mp4");
        assert_eq!(&events[drop.consumed..], &[Event::Resize(80, 24)]);
    }

    #[test]
    fn modified_shortcuts_are_never_treated_as_paths() {
        let events: Vec<_> = "samples/sample.mp4"
            .chars()
            .map(|c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)))
            .collect();
        assert!(dropped_path(&events).is_none());
    }
}
