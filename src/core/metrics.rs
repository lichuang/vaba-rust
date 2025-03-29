use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub struct Metrics {
    send: SendMetrics,
    recv: RecvMetrics,
}

struct RecvMetrics {
    pub proposal: AtomicU64,
    pub promote: AtomicU64,
}

struct SendMetrics {
    pub promote: AtomicU64,
    pub promote_resp: AtomicU64,
}

impl RecvMetrics {
    pub fn new() -> Self {
        Self {
            proposal: AtomicU64::new(0),
            promote: AtomicU64::new(0),
        }
    }
}

impl SendMetrics {
    pub fn new() -> Self {
        Self {
            promote: AtomicU64::new(0),
            promote_resp: AtomicU64::new(0),
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

    pub fn incr_recv_proposal(&mut self) {
        self.recv.proposal.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_recv_promote(&mut self) {
        self.recv.promote.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_promote(&mut self) {
        self.send.promote.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_promote_resp(&mut self) {
        self.send.promote_resp.fetch_add(1, Ordering::Relaxed);
    }
}
