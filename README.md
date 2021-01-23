<img width=950 src="./readme/postage-banner.svg">

**The feature-rich, portable async channel library** \> **[crates.io](https://crates.io/crates/postage)** \> **[docs.rs](https://docs.rs/postage/)**

## Why use Postage?
- Includes a **rich set of channels**
  - [mpsc](https://docs.rs/postage/latest/postage/mpsc/index.html) 
  **|** [broadcast](https://docs.rs/postage/latest/postage/broadcast/index.html) 
  **|** [watch](https://docs.rs/postage/latest/postage/watch/index.html) 
  **|** [oneshot](https://docs.rs/postage/latest/postage/oneshot/index.html) 
  **|** [barrier](https://docs.rs/postage/latest/postage/barrier/index.html)
- Works with **any executor**
  - Currently regressions are written for `tokio` and `async-std`.
- Includes **built-in [Stream](https://docs.rs/postage/latest/postage/stream/trait.Stream.html) and [Sink](https://docs.rs/postage/latest/postage/sink/trait.Sink.html) combinators**
  - Sinks can be chained and filtered.
  - Streams can be chained, filtered, mapped, and merged.
  - Sinks and streams can log their values, for easy app debugging.

Postage is in **beta** quality.  The functionality is implemented and has unit/integration test coverage.  But it needs to be tested on more hardware, and more operating systems.

## Channels
### postage::mpsc
Postage includes a fixed-capacity multi-producer, single-consumer channel.  The producer can be cloned, and the sender task is suspended if the channel becomes full.

### postage::broadcast
The broadcast channel provides multi-sender, multi-receiver message dispatch.  All receivers are sent every message.  The channel has a fixed capacity, and senders are suspended if the buffer is filled.

When a receiver is cloned, both receivers will be sent the same series of messages.

Senders also provide a `subscribe()` method which creates a receiver that will observe all messages sent *after* the call to subscribe.

### postage::watch
Watch channels can be used to asynchronously transmit state.  When receivers are created, they immediately recieve an initial value.  They will also recieve new values, but are not guaranteed to recieve *every* value.

Values transmitted over watch channels must implement Default.  A simple way to achieve this is to transmit `Option<T>`.

### postage::oneshot
Oneshot channels transmit a single value between a sender and a reciever.  Neither can be cloned.  If the sender drops, the receiver recieves a `None` value.

### postage::barrier
Barrier channels can be used to synchronize events, but do not transmit any data.  When the sender is dropped (or `tx.send(())` is called), the receiver is awoken.  This can be used to asynchronously coordinate actions between tasks.
