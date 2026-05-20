use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use crate::routing::health_probe::check_backend_health;

pub struct CircuitBreaker {
    registry: DashMap<SocketAddr, Arc<AtomicBool>>,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            registry: DashMap::new(),
        }
    }

    pub fn register(&self, addr: SocketAddr) {
        self.registry.insert(addr, Arc::new(AtomicBool::new(true)));
    }

    pub fn quarantine(&self, addr: &SocketAddr) {
        if let Some(state) = self.registry.get(addr) {
            state.store(false, Ordering::Release);
        }
    }

    pub fn is_healthy(&self, addr: &SocketAddr) -> bool {
        if let Some(state) = self.registry.get(addr) {
            state.load(Ordering::Acquire)
        } else {
            false
        }
    }

    pub async fn monitor(cb: Arc<CircuitBreaker>) {
        loop {
            for entry in cb.registry.iter() {
                let addr = *entry.key();
                let state = entry.value().clone();
                
                tokio::spawn(async move {
                    let is_up = check_backend_health(addr).await;
                    state.store(is_up, Ordering::Release);
                });
            }
            sleep(Duration::from_millis(500)).await;
        }
    }
}