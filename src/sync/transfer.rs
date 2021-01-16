use std::task::Waker;

use atomic::{Atomic, Ordering};

use crate::PollRecv;

use super::{
    notifier::Notifier,
    oneshot_cell::{OneshotCell, TryRecvError},
};

#[derive(Copy, Clone)]
enum State {
    Alive,
    Dead,
}

pub struct Transfer<T: Sized> {
    sender: Atomic<State>,
    receiver: Atomic<State>,
    value: OneshotCell<T>,
    notify_rx: Notifier,
}

impl<T> Transfer<T> {
    pub fn new() -> Self {
        Self {
            sender: Atomic::new(State::Alive),
            receiver: Atomic::new(State::Alive),
            value: OneshotCell::new(),
            notify_rx: Notifier::new(),
        }
    }

    pub fn send(&self, value: T) -> Result<(), T> {
        if let State::Dead = self.receiver.load(Ordering::Acquire) {
            return Err(value);
        }

        self.value.send(value)?;
        self.notify_rx.notify();

        Ok(())
    }

    pub fn recv(&self, waker: &Waker) -> PollRecv<T> {
        match self.value.try_recv() {
            Ok(value) => PollRecv::Ready(value),
            Err(TryRecvError::Pending) => {
                if let State::Dead = self.sender.load(Ordering::Acquire) {
                    return match self.value.try_recv() {
                        Ok(v) => PollRecv::Ready(v),
                        Err(TryRecvError::Pending) => PollRecv::Pending,
                        Err(TryRecvError::Closed) => PollRecv::Closed,
                    };
                }

                self.notify_rx.subscribe(waker.clone());

                match self.value.try_recv() {
                    Ok(v) => PollRecv::Ready(v),
                    Err(TryRecvError::Pending) => PollRecv::Pending,
                    Err(TryRecvError::Closed) => PollRecv::Closed,
                }
            }
            Err(TryRecvError::Closed) => PollRecv::Closed,
        }
    }

    pub fn sender_disconnect(&self) {
        self.sender.store(State::Dead, Ordering::Release);
    }

    pub fn receiver_disconnect(&self) {
        self.receiver.store(State::Dead, Ordering::Release);
    }
}
