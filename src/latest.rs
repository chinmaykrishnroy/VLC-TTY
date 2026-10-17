//! A one-item mailbox: slow consumers receive the newest work, never a backlog.
use std::sync::{Arc, Condvar, Mutex};

struct State<T> {
    value: Option<T>,
    closed: bool,
}
struct Shared<T> {
    state: Mutex<State<T>>,
    ready: Condvar,
}
pub struct Sender<T>(Arc<Shared<T>>);
pub struct Receiver<T>(Arc<Shared<T>>);

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            value: None,
            closed: false,
        }),
        ready: Condvar::new(),
    });
    (Sender(Arc::clone(&shared)), Receiver(shared))
}

impl<T> Shared<T> {
    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        state.value = None;
        self.ready.notify_all();
    }
}

impl<T> Sender<T> {
    pub fn replace(&self, value: T) -> Result<bool, T> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return Err(value);
        }
        let replaced = state.value.replace(value).is_some();
        self.0.ready.notify_one();
        Ok(replaced)
    }
    pub fn clear(&self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .value = None;
    }
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Option<T> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while state.value.is_none() && !state.closed {
            state = self
                .0
                .ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        state.value.take()
    }
    pub fn try_take(&self) -> Option<T> {
        self.0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .value
            .take()
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.0.close();
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_reports_discarded_pending_work() {
        let (sender, receiver) = channel();
        assert_eq!(sender.replace(1), Ok(false));
        assert_eq!(sender.replace(2), Ok(true));
        assert_eq!(receiver.try_take(), Some(2));
    }
    #[test]
    fn pending_frames_are_replaced_without_waiting_for_consumer() {
        let (sender, receiver) = channel();
        for serial in 1..=1000 {
            sender.replace(serial).unwrap();
        }
        assert_eq!(receiver.try_take(), Some(1000));
        assert_eq!(receiver.try_take(), None);
    }
    #[test]
    fn shutdown_wakes_a_waiting_worker() {
        let (sender, receiver) = channel::<()>();
        let worker = std::thread::spawn(move || receiver.recv());
        drop(sender);
        assert_eq!(worker.join().unwrap(), None);
    }
    #[test]
    fn closed_receiver_rejects_new_work() {
        let (sender, receiver) = channel();
        drop(receiver);
        assert_eq!(sender.replace(42), Err(42));
    }
}
