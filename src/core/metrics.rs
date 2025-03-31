use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub struct Metrics {
    recv_proposal: AtomicU64,

    send_promote: AtomicU64,
    send_ack: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            recv_proposal: AtomicU64::new(0),
            send_promote: AtomicU64::new(0),
            send_ack: AtomicU64::new(0),
        }
    }

    pub fn incr_recv_proposal(&self) {
        self.recv_proposal.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_promote(&self) {
        self.send_promote.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_ack(&self) {
        self.send_ack.fetch_add(1, Ordering::Relaxed);
    }
}
