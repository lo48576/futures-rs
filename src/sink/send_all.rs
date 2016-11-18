use {Poll, Async, Future, AsyncSink};
use stream::{Stream, Fuse};
use sink::Sink;

/// Future for the `Sink::send_all` combinator, which sends a stream of values
/// to a sink and then waits until the sink has fully flushed those values.
#[must_use = "futures do nothing unless polled"]
pub struct SendAll<T, U: Stream> {
    sink: Option<T>,
    stream: Option<Fuse<U>>,
    buffered: Option<U::Item>,
}

pub fn new<T, U>(sink: T, stream: U) -> SendAll<T, U>
    where T: Sink,
          U: Stream<Item = T::SinkItem>,
          T::SinkError: From<U::Error>,
{
    SendAll {
        sink: Some(sink),
        stream: Some(stream.fuse()),
        buffered: None,
    }
}

impl<T, U> SendAll<T, U>
    where T: Sink,
          U: Stream<Item = T::SinkItem>,
          T::SinkError: From<U::Error>,
{
    fn sink_mut(&mut self) -> &mut T {
        self.sink.as_mut().take().expect("Attempted to poll SendAll after completion")
    }

    fn stream_mut(&mut self) -> &mut Fuse<U> {
        self.stream.as_mut().take()
            .expect("Attempted to poll SendAll after completion")
    }

    fn take_result(&mut self) -> (T, U) {
        let sink = self.sink.take()
            .expect("Attempted to poll Forward after completion");
        let fuse = self.stream.take()
            .expect("Attempted to poll Forward after completion");
        return (sink, fuse.into_inner());
    }

    fn try_start_send(&mut self, item: U::Item) -> Poll<(), T::SinkError> {
        debug_assert!(self.buffered.is_none());
        if let AsyncSink::NotReady(item) = try!(self.sink_mut().start_send(item)) {
            self.buffered = Some(item);
            return Ok(Async::NotReady)
        }
        Ok(Async::Ready(()))
    }
}

impl<T, U> Future for SendAll<T, U>
    where T: Sink,
          U: Stream<Item = T::SinkItem>,
          T::SinkError: From<U::Error>,
{
    type Item = (T, U);
    type Error = T::SinkError;

    fn poll(&mut self) -> Poll<(T, U), T::SinkError> {
        // If we've got an item buffered already, we need to write it to the
        // sink before we can do anything else
        if let Some(item) = self.buffered.take() {
            try_ready!(self.try_start_send(item))
        }

        loop {
            try!(self.sink_mut().poll_complete());

            if let Some(item) = try_ready!(self.stream_mut().poll()) {
                try_ready!(self.try_start_send(item))
            } else {
                // we're done pushing the stream, but want to block on flushing the
                // sink
                try_ready!(self.sink_mut().poll_complete());

                // now everything's emptied, so return the sink for further use
                return Ok(Async::Ready(self.take_result()))
            }
        }
    }
}
