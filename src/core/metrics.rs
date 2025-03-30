use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub struct Metrics {
    send: SendMetrics,
    recv: RecvMetrics,
}

struct RecvMetrics {
    pub proposal: AtomicU64,
    pub promote: AtomicU64,
    pub done: AtomicU64,
}

struct SendMetrics {
    pub promote: AtomicU64,
    pub promote_ack: AtomicU64,
    pub done: AtomicU64,
}

impl RecvMetrics {
    pub fn new() -> Self {
        Self {
            proposal: AtomicU64::new(0),
            promote: AtomicU64::new(0),
            done: AtomicU64::new(0),
        }
    }
}

impl SendMetrics {
    pub fn new() -> Self {
        Self {
            promote: AtomicU64::new(0),
            promote_ack: AtomicU64::new(0),
            done: AtomicU64::new(0),
        }
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            recv: RecvMetrics::new(),
            send: SendMetrics::new(),
        }
    }

    pub fn incr_recv_proposal(&self) {
        self.recv.proposal.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_recv_promote(&self) {
        self.recv.promote.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_recv_done(&self) {
        self.recv.done.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_promote(&self) {
        self.send.promote.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_promote_ack(&self) {
        self.send.promote_ack.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_done(&self) {
        self.send.done.fetch_add(1, Ordering::Relaxed);
    }
}
