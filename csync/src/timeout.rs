use std::time::{Duration, Instant};

use futures::{Stream, Async, Poll};
use tokio::timer::Interval;

pub struct TimeoutStream<S: Stream> {
    got_something: bool,
    stream: S,
    interval: Interval,
}

impl<S: Stream> TimeoutStream<S> {
    pub fn new(s: S, timeout: Duration) -> TimeoutStream<S> {
        TimeoutStream {
            got_something: false,
            stream: s,
            interval: Interval::new(Instant::now() + timeout, timeout),
        }
    }
}

#[derive(Debug)]
pub enum Error<T> {
    Timeout,
    Timer(<Interval as Stream>::Error),
    Other(T),
}

impl<S: Stream> Stream for TimeoutStream<S> {
    type Item = S::Item;
    type Error = Error<S::Error>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.stream.poll() {
            Ok(Async::NotReady) => {},
            Ok(Async::Ready(t)) => {
                self.got_something = true;
                return Ok(Async::Ready(t))
            },
            Err(e) => return Err(Error::Other(e)),
        }

        match self.interval.poll() {
            Ok(Async::Ready(Some(_))) if self.got_something => {
                self.got_something = false;
                // poll until NotReady to make Instant register itself again
                // This may result in an infinite loop if the duration is very low, but who cares.
                loop {
                    match self.interval.poll() {
                        Ok(Async::NotReady) => break,
                        Ok(Async::Ready(_)) => continue,
                        Err(e) => return Err(Error::Timer(e)),
                    }
                }
            }
            Ok(Async::Ready(Some(_))) => return Err(Error::Timeout),
            Ok(Async::Ready(None)) => unreachable!(),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => return Err(Error::Timer(e)),
        }
        Ok(Async::NotReady)
    }
}