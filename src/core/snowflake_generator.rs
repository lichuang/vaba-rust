use chrono::Utc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::base::NodeId;

use super::IdempotentId;

// 42位时间戳 + 10位节点ID + 12位序列号
pub struct SnowflakeGenerator {
    node_id: NodeId,
    sequence: AtomicU32,
    last_timestamp: AtomicU64,
}

impl SnowflakeGenerator {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            sequence: AtomicU32::new(0),
            last_timestamp: AtomicU64::new(0),
        }
    }

    pub fn timestamp(id: IdempotentId) -> u64 {
        id >> 22
    }

    pub fn generate_with_timestamp(&self, timestamp: u64) -> IdempotentId {
        let mut timestamp = timestamp;
        let mut sequence = self.sequence.load(Ordering::Relaxed);

        if timestamp == self.last_timestamp.load(Ordering::Acquire) {
            sequence = (sequence + 1) % 4096; // 12位序列号
            if sequence == 0 {
                timestamp = Self::wait_next_millis(timestamp);
            }
            self.sequence.store(sequence, Ordering::Relaxed);
        } else {
            sequence = 0;
            self.sequence.store(sequence, Ordering::Relaxed);
        }

        self.last_timestamp.store(timestamp, Ordering::Release);

        (timestamp << 22) | (self.node_id << 12) | (sequence as u64)
    }

    pub fn generate(&self) -> IdempotentId {
        let timestamp = Utc::now().timestamp_millis() as u64;

        self.generate_with_timestamp(timestamp)
    }

    fn wait_next_millis(last_timestamp: u64) -> u64 {
        let mut timestamp = Utc::now().timestamp_millis() as u64;
        while timestamp <= last_timestamp {
            std::thread::sleep(std::time::Duration::from_millis(1));
            timestamp = Utc::now().timestamp_millis() as u64;
        }
        timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::SnowflakeGenerator;
    use anyhow::Result;
    use chrono::Utc;

    #[test]
    fn test_generate_idempotent_id() -> Result<()> {
        let generator = SnowflakeGenerator::new(11);
        let timestamp = Utc::now().timestamp_millis() as u64;
        let id = generator.generate_with_timestamp(timestamp);
        assert_eq!(timestamp, SnowflakeGenerator::timestamp(id));

        // generate id with the same timestamp again, assert the new id is bigger
        let id1 = generator.generate_with_timestamp(timestamp);
        assert_eq!(timestamp, SnowflakeGenerator::timestamp(id1));
        assert!(id1 > id);

        // generate id with bigger timestamp again, assert the new id is bigger
        let id2 = generator.generate_with_timestamp(timestamp + 1);
        assert_eq!(timestamp + 1, SnowflakeGenerator::timestamp(id2));
        assert!(id2 > id);
        assert!(id2 > id1);

        Ok(())
    }
}
