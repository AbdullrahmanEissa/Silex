use std::sync::atomic::{AtomicU64, Ordering};

pub struct AtomicMetrics {
    requests: AtomicU64,
    total_latency_ms: AtomicU64,
}

impl AtomicMetrics {
    pub fn new() -> Self {
        Self {
            requests: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
        }
    }

    pub fn record_request(&self, latency_ms: u64) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ms.fetch_add(latency_ms, Ordering::Relaxed);
    }

    pub fn get_requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn get_total_latency_ms(&self) -> u64 {
        self.total_latency_ms.load(Ordering::Relaxed)
    }
}