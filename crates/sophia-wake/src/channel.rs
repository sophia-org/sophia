//! Bounded std channels with a private, cancellable consumer wake.
use crate::{Notifier, Wake, WakeSlot};
use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

pub struct Sender<T> {
    sender: Option<mpsc::SyncSender<T>>,
    wake: Notifier,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            wake: self.wake.clone(),
        }
    }
}

impl<T> Sender<T> {
    pub fn try_send(&self, value: T) -> Result<(), mpsc::TrySendError<T>> {
        self.sender.as_ref().expect("live sender").try_send(value)?;
        self.wake.notify();
        Ok(())
    }

    pub fn send(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        self.sender.as_ref().expect("live sender").send(value)?;
        self.wake.notify();
        Ok(())
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Disconnect must be visible before the sleeping receiver wakes.
        drop(self.sender.take());
        self.wake.notify();
    }
}

pub struct Receiver<T> {
    receiver: Option<mpsc::Receiver<T>>,
    wake: Option<Wake>,
    capacity_wake: WakeSlot,
}

impl<T> Receiver<T> {
    pub fn notifier(&self) -> Option<Notifier> {
        self.wake.as_ref().map(Wake::notifier)
    }

    pub fn set_capacity_wake(&self, notifier: Notifier) {
        self.capacity_wake.set(notifier);
    }

    pub fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        let result = self.receiver.as_ref().expect("live receiver").try_recv();
        if result.is_ok() {
            self.capacity_wake.notify();
        }
        result
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, mpsc::RecvTimeoutError> {
        let result = self
            .receiver
            .as_ref()
            .expect("live receiver")
            .recv_timeout(timeout);
        if result.is_ok() {
            self.capacity_wake.notify();
        }
        result
    }

    pub fn recv(&self) -> Result<T, mpsc::RecvError> {
        let result = self.receiver.as_ref().expect("live receiver").recv();
        if result.is_ok() {
            self.capacity_wake.notify();
        }
        result
    }

    pub fn try_iter(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }

    /// `None` means cancellation or the last sender disconnected. The caller
    /// owns cancellation: store its flag, then ring `notifier()` before join.
    pub fn recv_until_stopped(&self, stop: &AtomicBool) -> io::Result<Option<T>> {
        let Some(wake) = &self.wake else {
            // Compatibility for callers supplying bare std receivers. Their
            // senders cannot ring, so they retain their original timed wait.
            while !stop.load(Ordering::Acquire) {
                match self.recv_timeout(Duration::from_millis(10)) {
                    Ok(value) => return Ok(Some(value)),
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            return Ok(None);
        };
        loop {
            wake.clear()?;
            if stop.load(Ordering::Acquire) {
                return Ok(None);
            }
            match self.try_recv() {
                Ok(value) => return Ok(Some(value)),
                Err(mpsc::TryRecvError::Disconnected) => return Ok(None),
                Err(mpsc::TryRecvError::Empty) => {
                    wake.wait(None)?;
                }
            }
        }
    }
}

impl<T> From<mpsc::Receiver<T>> for Receiver<T> {
    fn from(receiver: mpsc::Receiver<T>) -> Self {
        Self {
            receiver: Some(receiver),
            wake: None,
            capacity_wake: WakeSlot::default(),
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // A deferred producer must observe disconnect before it wakes.
        drop(self.receiver.take());
        self.capacity_wake.notify();
    }
}

pub fn bounded<T>(capacity: usize) -> io::Result<(Sender<T>, Receiver<T>)> {
    // Notification follows publication. A rendezvous send cannot publish
    // until the receiver runs, so it cannot wake a receiver already asleep.
    if capacity == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "notifying channels require positive capacity",
        ));
    }
    let wake = Wake::new()?;
    let (sender, receiver) = mpsc::sync_channel(capacity);
    Ok((
        Sender {
            sender: Some(sender),
            wake: wake.notifier(),
        },
        Receiver {
            receiver: Some(receiver),
            wake: Some(wake),
            capacity_wake: WakeSlot::default(),
        },
    ))
}
