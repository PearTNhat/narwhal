// p2p/src/receiver.rs
use tokio::sync::mpsc;
use log::{info, warn};
use bytes::Bytes;
use serde::de::DeserializeOwned;

pub struct P2PReceiver;

impl P2PReceiver {
    /// Spawn một receiver P2P đơn giản chỉ để test
    /// Nó nhận message từ kênh MPSC và gửi vào kênh khác để xử lý
    pub fn spawn<T>(
        mut rx_swarm: mpsc::Receiver<Bytes>,
        tx_handler: mpsc::Sender<T>,
    ) where
        T: DeserializeOwned + Send + 'static,
    {
        tokio::spawn(async move {
            info!("🟢 P2P Receiver spawned and listening for messages from Swarm");
            while let Some(bytes) = rx_swarm.recv().await {
                info!("📦 P2P Receiver got {} bytes from Swarm", bytes.len());
                
                // Deserialize message
                match bincode::deserialize::<T>(&bytes) {
                    Ok(message) => {
                        info!("✅ Successfully deserialized P2P message");
                        if tx_handler.send(message).await.is_err() {
                            warn!("❌ Failed to send message to handler channel");
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("❌ Failed to deserialize P2P message: {}", e);
                    }
                }
            }
            warn!("⚠️ P2P Receiver channel closed");
        });
    }
}