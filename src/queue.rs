//! Playback order remains stable while a frontend browses other folders.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Default)]
pub struct PlaybackQueue {
    sources: Vec<String>,
    position: Option<usize>,
    pub repeat: RepeatMode,
}

impl PlaybackQueue {
    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    /// Retain the current queue when selecting an existing item. A new source
    /// seeds a new queue; failed opens never change the active queue position.
    pub fn select(&mut self, source: &str, siblings: Vec<String>) {
        if let Some(index) = self.sources.iter().position(|item| item == source) {
            self.position = Some(index);
            return;
        }
        self.sources = siblings;
        if !self.sources.iter().any(|item| item == source) {
            self.sources.push(source.into());
        }
        self.position = self.sources.iter().position(|item| item == source);
    }

    pub fn adjacent(&self, step: isize, automatic: bool) -> Option<&str> {
        let position = self.position?;
        if automatic && self.repeat == RepeatMode::One {
            return self.sources.get(position).map(String::as_str);
        }
        let next = position.checked_add_signed(step);
        let index = match next.filter(|&index| index < self.sources.len()) {
            Some(index) => index,
            None if self.repeat == RepeatMode::All => {
                if step < 0 {
                    self.sources.len().checked_sub(1)?
                } else {
                    0
                }
            }
            None => return None,
        };
        self.sources.get(index).map(String::as_str)
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_preserves_queue_and_stops_at_boundaries() {
        let mut queue = PlaybackQueue::default();
        queue.select("a", vec!["a".into(), "b".into()]);
        assert_eq!(queue.adjacent(-1, false), None);
        assert_eq!(queue.adjacent(1, false), Some("b"));
        queue.select("b", vec![]);
        assert_eq!(queue.sources(), &["a", "b"]);
        assert_eq!(queue.adjacent(1, true), None);
    }

    #[test]
    fn repeat_policy_distinguishes_manual_next_from_repeating_current() {
        let mut queue = PlaybackQueue::default();
        queue.select("b", vec!["a".into(), "b".into()]);
        queue.cycle_repeat();
        assert_eq!(queue.adjacent(1, true), Some("a"));
        queue.cycle_repeat();
        assert_eq!(queue.adjacent(1, true), Some("b"));
        assert_eq!(queue.adjacent(-1, false), Some("a"));
        assert_eq!(queue.adjacent(1, false), None);
    }
}
