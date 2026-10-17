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
    original: Vec<String>,
    order: Vec<usize>,
    position: Option<usize>,
    pub repeat: RepeatMode,
    pub shuffled: bool,
}

impl PlaybackQueue {
    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    /// Retain the current queue when selecting an existing item. A new source
    /// seeds a new queue; failed opens never change the active queue position.
    pub fn select(&mut self, source: &str, siblings: Vec<String>) {
        if self
            .position
            .and_then(|index| self.sources.get(index))
            .is_some_and(|current| current == source)
        {
            return;
        }
        if let Some(index) = self.sources.iter().position(|item| item == source) {
            self.position = Some(index);
            return;
        }
        self.sources = siblings;
        if !self.sources.iter().any(|item| item == source) {
            self.sources.push(source.into());
        }
        self.position = self.sources.iter().position(|item| item == source);
        self.original = self.sources.clone();
        self.order = (0..self.sources.len()).collect();
        if self.shuffled {
            self.shuffle_with(&mut rand::thread_rng());
        }
    }

    pub fn replace(&mut self, sources: Vec<String>) {
        self.sources = sources;
        self.original = self.sources.clone();
        self.order = (0..self.sources.len()).collect();
        self.position = if self.sources.is_empty() {
            None
        } else {
            Some(0)
        };
        if self.shuffled {
            self.shuffle_with(&mut rand::thread_rng());
        }
    }

    fn shuffle_with(&mut self, random: &mut impl rand::Rng) {
        use rand::seq::SliceRandom;
        if let Some(position) = self.position {
            self.order.swap(0, position);
            self.order[1..].shuffle(random);
            self.sources = self
                .order
                .iter()
                .map(|&index| self.original[index].clone())
                .collect();
            self.position = Some(0);
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffled = !self.shuffled;
        if self.shuffled {
            self.shuffle_with(&mut rand::thread_rng());
        } else {
            let current = self
                .position
                .and_then(|index| self.order.get(index))
                .copied();
            self.sources = self.original.clone();
            self.order = (0..self.sources.len()).collect();
            self.position = current;
        }
    }

    pub fn adjacent_index(&self, step: isize, automatic: bool) -> Option<usize> {
        let position = self.position?;
        if automatic && self.repeat == RepeatMode::One {
            return Some(position);
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
        Some(index)
    }

    pub fn set_position(&mut self, index: usize) {
        if index < self.sources.len() {
            self.position = Some(index);
        }
    }

    #[cfg(test)]
    pub fn adjacent(&self, step: isize, automatic: bool) -> Option<&str> {
        self.adjacent_index(step, automatic)
            .and_then(|index| self.sources.get(index))
            .map(String::as_str)
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
    fn shuffle_restores_the_same_duplicate_occurrence() {
        let mut queue = PlaybackQueue::default();
        queue.replace(vec!["a".into(), "a".into(), "b".into()]);
        queue.set_position(1);
        queue.toggle_shuffle();
        queue.toggle_shuffle();
        assert_eq!(queue.adjacent(1, false), Some("b"));
    }
    #[test]
    fn shuffle_preserves_current_track_and_can_restore_explicit_order() {
        let mut queue = PlaybackQueue::default();
        queue.replace(vec!["a".into(), "b".into(), "c".into(), "d".into()]);
        queue.select("c", vec![]);
        queue.toggle_shuffle();
        assert_eq!(queue.sources()[0], "c");
        let mut sorted = queue.sources().to_vec();
        sorted.sort();
        assert_eq!(sorted, ["a", "b", "c", "d"]);
        queue.toggle_shuffle();
        assert_eq!(queue.sources(), &["a", "b", "c", "d"]);
        assert_eq!(queue.adjacent(1, false), Some("d"));
    }

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
