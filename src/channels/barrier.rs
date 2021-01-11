use std::sync::Arc;

use atomic::{Atomic, Ordering};
use static_assertions::{assert_impl_all, assert_not_impl_all};

use crate::{sync::notifier::Notifier, PollRecv, PollSend, Sink, Stream};

pub fn channel() -> (Sender, Receiver) {
    let shared = Arc::new(Shared {
        state: Atomic::new(State::Pending),
        notify_rx: Notifier::new(),
    });

    let sender = Sender {
        shared: shared.clone(),
    };

    let receiver = Receiver { shared };

    (sender, receiver)
}

pub struct Sender {
    pub(in crate::channels::barrier) shared: Arc<Shared>,
}

assert_impl_all!(Sender: Send);
assert_not_impl_all!(Sender: Clone);

impl Sink for Sender {
    type Item = ();

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _value: (),
    ) -> crate::PollSend<Self::Item> {
        match self.shared.state.load(Ordering::Acquire) {
            State::Pending => {
                self.shared.close();
                PollSend::Ready
            }
            State::Closed => PollSend::Rejected(()),
        }
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.shared.close();
    }
}

#[derive(Clone)]
pub struct Receiver {
    pub(in crate::channels::barrier) shared: Arc<Shared>,
}

assert_impl_all!(Receiver: Send, Clone);

#[derive(Copy, Clone)]
enum State {
    Pending,
    Closed,
}

struct Shared {
    state: Atomic<State>,
    notify_rx: Notifier,
}

impl Shared {
    pub fn close(&self) {
        self.state.store(State::Closed, Ordering::Release);
        self.notify_rx.notify();
    }
}

impl Stream for Receiver {
    type Item = ();

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        match self.shared.state.load(Ordering::Acquire) {
            State::Pending => {
                self.shared.notify_rx.subscribe(cx.waker().clone());
                PollRecv::Pending
            }
            State::Closed => PollRecv::Ready(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::{new_count_waker, noop_context, panic_context};

    use super::channel;

    #[test]
    fn send_accepted() {
        let mut cx = noop_context();
        let (mut tx, _rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));
        assert_eq!(
            PollSend::Rejected(()),
            Pin::new(&mut tx).poll_send(&mut cx, ())
        );
    }

    #[test]
    fn send_recv() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn sender_disconnect() {
        let mut cx = noop_context();
        let (tx, mut rx) = channel();

        drop(tx);

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn send_then_disconnect() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        drop(tx);

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn receiver_disconnect() {
        let mut cx = noop_context();
        let (mut tx, rx) = channel();

        drop(rx);

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));
    }

    #[test]
    fn receiver_clone() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();
        let mut rx2 = rx.clone();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx2).poll_recv(&mut cx));
    }

    #[test]
    fn receiver_send_then_clone() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        let mut rx2 = rx.clone();

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx2).poll_recv(&mut cx));
    }

    #[test]
    fn wake_receiver() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel();

        let (w, w_count) = new_count_waker();
        let mut w_context = Context::from_waker(&w);

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w_context)
        );

        assert_eq!(0, w_count.get());

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        assert_eq!(1, w_count.get());

        assert_eq!(
            PollSend::Rejected(()),
            Pin::new(&mut tx).poll_send(&mut cx, ())
        );

        assert_eq!(1, w_count.get());
    }

    #[test]
    fn wake_receiver_on_disconnect() {
        let (tx, mut rx) = channel();

        let (w1, w1_count) = new_count_waker();
        let mut w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context)
        );

        assert_eq!(0, w1_count.get());

        drop(tx);

        assert_eq!(1, w1_count.get());
    }
}

#[cfg(test)]
mod tokio_tests {
    use std::time::Duration;

    use tokio::{task::spawn, time::timeout};

    use crate::{test::CHANNEL_TEST_RECEIVERS, Sink, Stream};

    use super::Receiver;

    async fn assert_rx(mut rx: Receiver) {
        if let Err(_e) = timeout(Duration::from_millis(100), rx.recv()).await {
            panic!("Timeout waiting for barrier");
        }
    }

    #[tokio::test]
    async fn simple() {
        let (mut tx, rx) = super::channel();

        tx.send(()).await.expect("Should send message");

        assert_rx(rx).await;
    }

    #[tokio::test]
    async fn simple_drop() {
        let (tx, rx) = super::channel();

        drop(tx);

        assert_rx(rx).await;
    }

    #[tokio::test]
    async fn multi_receiver() {
        let (tx, rx) = super::channel();

        let handles = (0..CHANNEL_TEST_RECEIVERS).map(|_| {
            let rx2 = rx.clone();

            spawn(async move {
                assert_rx(rx2).await;
            })
        });

        drop(tx);

        for handle in handles {
            handle.await.expect("Assertion failure");
        }
    }
}

#[cfg(test)]
mod async_std_tests {
    use std::time::Duration;

    use async_std::{future::timeout, task::spawn};

    use crate::{test::CHANNEL_TEST_RECEIVERS, Sink, Stream};

    use super::Receiver;

    async fn assert_rx(mut rx: Receiver) {
        if let Err(_e) = timeout(Duration::from_millis(100), rx.recv()).await {
            panic!("Timeout waiting for barrier");
        }
    }

    #[async_std::test]
    async fn simple() {
        let (mut tx, rx) = super::channel();

        tx.send(()).await.expect("Should send message");

        assert_rx(rx).await;
    }

    #[async_std::test]
    async fn simple_drop() {
        let (tx, rx) = super::channel();

        drop(tx);

        assert_rx(rx).await;
    }

    #[async_std::test]
    async fn multi_receiver() {
        let (tx, rx) = super::channel();

        let handles = (0..CHANNEL_TEST_RECEIVERS).map(|_| {
            let rx2 = rx.clone();

            spawn(async move {
                assert_rx(rx2).await;
            })
        });

        drop(tx);

        for handle in handles {
            handle.await;
        }
    }
}
