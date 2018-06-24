use std::time::{Duration, Instant};
use std::collections::VecDeque;
use std::mem;

use tokio::timer::Delay;
use tokio::prelude::task;
use futures::{Async, Stream, Future};

pub struct CongestionInfo {
    rtt_start: Option<Instant>,
    #[allow(unused)]
    rtts: VecDeque<Duration>,
    rtt: Duration,
    last_ipt: Option<Instant>,
    ipts: VecDeque<Duration>,
    ipt: Duration,
    last_notify: Instant,
    packets_since_last_notify: u32,
    delay: Option<Delay>,
    done: bool,
}

impl CongestionInfo {
    pub fn new() -> CongestionInfo {
        CongestionInfo {
            rtt_start: None,
            rtts: VecDeque::with_capacity(10),
            rtt: Duration::from_millis(0),
            last_ipt: None,
            ipts: VecDeque::with_capacity(10),
            ipt: Duration::from_millis(0),
            last_notify: Instant::now(),
            packets_since_last_notify: 0,
            delay: None,
            done: false,
        }
    }

    pub fn start_rtt(&mut self) {
        // TODO: handle case where RTT response got dropped / a new start_rtt was fired before the ACK was received
        if self.rtt_start.is_some() {
            panic!("Called `start_rtt` with ongoing RTT");
        }
        self.rtt_start = Some(Instant::now());
    }

    pub fn stop_rtt(&mut self) {
        // TODO: moving average
        match self.rtt_start.take() {
            Some(rtt_start) => {
                self.rtt = rtt_start.elapsed();
                self.rtts.push_back(self.rtt);
            },
            None => panic!("Called `stop_rtt` without a previous `start_rtt`")
        }
    }

    pub fn is_rtt_running(&self) -> bool {
        self.rtt_start.is_some()
    }

    pub fn rtt(&self) -> Duration {
        if self.rtts.len() == 0 {
            panic!("Called rtt without having performed an rtt measurement");
        }
        self.rtt
    }

    pub fn ipt_packet(&mut self) {
        // IPT calculation
        if let None = self.last_ipt {
            self.last_ipt = Some(Instant::now());
            return;
        }
        let now = Instant::now();
        let old = mem::replace(&mut self.last_ipt, Some(now));
        let diff = now.duration_since(old.unwrap());
        if self.ipts.len() == 10 {
            self.ipt -= self.ipts.pop_front().unwrap() / 10;
        } else {
            self.ipt = self.ipt * self.ipts.len() as u32 / (self.ipts.len() as u32 + 1);
        }
        self.ipts.push_back(diff);
        self.ipt += diff / self.ipts.len() as u32;

        self.update_delay();

        // check packets since last notify and notify task if needed
        self.packets_since_last_notify += 1;
        if self.packets_since_last_notify > self.num_packets() {
            task::current().notify();
        }
    }

    pub fn num_packets(&self) -> u32 {
        let ipt = self.ipt.as_secs() as f64 + self.ipt.subsec_nanos() as f64 * 1e-9;
        let pps = 1.0 / ipt;
        let num_packets = pps / (pps + 1.0).ln();
        num_packets as u32
    }

    fn update_delay(&mut self) {
        let time_left = self.ipt * self.num_packets() + self.rtt;
        if let Some(ref mut delay) = self.delay {
            delay.reset(self.last_notify + time_left);
        } else {
            self.delay = Some(Delay::new(self.last_notify + time_left));
        }
    }

    pub fn shutdown(&mut self) {
        self.done = true;
    }
}

impl Stream for CongestionInfo {
    type Item = ();
    type Error = <Delay as Future>::Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        if self.done {
            return Ok(Async::NotReady);
        }
        if self.delay.is_none() {
            return Ok(Async::NotReady);
        }
        match self.delay.as_mut().unwrap().poll() {
            Ok(Async::NotReady) if self.packets_since_last_notify >= self.num_packets() => {
                self.packets_since_last_notify = 0;
                self.last_notify = Instant::now();
                self.update_delay();
                return Ok(Async::Ready(Some(())));
            }
            Ok(Async::Ready(())) => {
                self.packets_since_last_notify = 0;
                self.last_notify = Instant::now();
                self.update_delay();
                return Ok(Async::Ready(Some(())));
            }
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => Err(e)?
        }
    }
}