// src/state.rs - Simplified for Narwhal integration
use std::sync::Arc;

pub struct AppState {
    pub local_peer_id: String,
}

impl AppState {
    pub fn new(local_peer_id: String) -> Arc<Self> {
        Arc::new(Self { local_peer_id })
    }
}