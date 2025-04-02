use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    recv_proposal: AtomicU64,

    send_promote: AtomicU64,
    send_ack: AtomicU64,
    send_done: AtomicU64,
    send_skip_share: AtomicU64,
    send_skip: AtomicU64,
    send_share: AtomicU64,
    send_view_change: AtomicU64,
}

impl Clone for Metrics {
    fn clone(&self) -> Self {
        Metrics {
            recv_proposal: AtomicU64::new(self.recv_proposal.load(Ordering::Relaxed)),
            send_promote: AtomicU64::new(self.send_promote.load(Ordering::Relaxed)),
            send_ack: AtomicU64::new(self.send_ack.load(Ordering::Relaxed)),
            send_done: AtomicU64::new(self.send_done.load(Ordering::Relaxed)),
            send_skip_share: AtomicU64::new(self.send_skip_share.load(Ordering::Relaxed)),
            send_skip: AtomicU64::new(self.send_skip.load(Ordering::Relaxed)),
            send_share: AtomicU64::new(self.send_share.load(Ordering::Relaxed)),
            send_view_change: AtomicU64::new(self.send_view_change.load(Ordering::Relaxed)),
        }
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            recv_proposal: AtomicU64::new(0),
            send_promote: AtomicU64::new(0),
            send_ack: AtomicU64::new(0),
            send_done: AtomicU64::new(0),
            send_skip_share: AtomicU64::new(0),
            send_skip: AtomicU64::new(0),
            send_share: AtomicU64::new(0),
            send_view_change: AtomicU64::new(0),
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

    pub fn incr_send_done(&self) {
        self.send_done.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_skip_share(&self) {
        self.send_skip_share.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_skip(&self) {
        self.send_skip.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_share(&self) {
        self.send_share.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_send_view_change(&self) {
        self.send_view_change.fetch_add(1, Ordering::Relaxed);
    }
}
