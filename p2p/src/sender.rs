// p2p/src/sender.rs
use tokio::sync::mpsc;
use log::{info, warn};
use bytes::Bytes;
use serde::Serialize;

/// P2P Sender để gửi message qua Swarm
pub struct P2PSender {
    tx_to_swarm: mpsc::Sender<Bytes>,
}

impl P2PSender {
    pub fn new(tx_to_swarm: mpsc::Sender<Bytes>) -> Self {
        Self { tx_to_swarm }
    }

    /// Gửi một message đã được serialize
    pub async fn send<T: Serialize>(&self, message: &T) -> Result<(), Box<dyn std::error::Error>> {
        let serialized = bincode::serialize(message)?;
        info!("📤 P2P Sender sending {} bytes", serialized.len());
        
        self.tx_to_swarm
            .send(Bytes::from(serialized))
            .await
            .map_err(|e| format!("Failed to send to swarm: {}", e))?;
        
        info!("✅ P2P message sent successfully");
        Ok(())
    }
}

impl Clone for P2PSender {
    fn clone(&self) -> Self {
        Self {
            tx_to_swarm: self.tx_to_swarm.clone(),
        }
    }
}
