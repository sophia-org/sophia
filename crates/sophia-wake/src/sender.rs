//! Existing channel endpoints with publication and disconnect notifications.
use crate::WakeSlot;
use std::sync::mpsc::{self, SendError, TrySendError};

pub struct SignalSender<T> {
    sender: Option<mpsc::SyncSender<T>>,
    wake: WakeSlot,
}

impl<T> From<mpsc::SyncSender<T>> for SignalSender<T> {
    fn from(sender: mpsc::SyncSender<T>) -> Self {
        Self::new(sender, WakeSlot::default())
    }
}

impl<T> Clone for SignalSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            wake: self.wake.clone(),
        }
    }
}

impl<T> SignalSender<T> {
    pub fn notify(&self) {
        self.wake.notify();
    }
    pub fn new(sender: mpsc::SyncSender<T>, wake: WakeSlot) -> Self {
        Self {
            sender: Some(sender),
            wake,
        }
    }

    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        self.sender.as_ref().expect("live sender").send(value)?;
        self.wake.notify();
        Ok(())
    }

    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        self.sender.as_ref().expect("live sender").try_send(value)?;
        self.wake.notify();
        Ok(())
    }
}

impl<T> Drop for SignalSender<T> {
    fn drop(&mut self) {
        drop(self.sender.take());
        self.wake.notify();
    }
}
