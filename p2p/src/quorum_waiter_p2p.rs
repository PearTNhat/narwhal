// p2p/src/quorum_waiter_p2p.rs
// QuorumWaiter implementation using libp2p request-response

use crate::req_res_protocol::WorkerP2pEvent;
use config::{Committee, Stake};
use crypto::PublicKey;
use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};
use log::{info, warn};

/// Message từ BatchMaker gửi đến QuorumWaiter
#[derive(Debug)]
pub struct QuorumWaiterMessage {
    /// Batch data đã serialize
    pub batch: Vec<u8>,
    /// Batch digest/ID
    pub batch_id: Vec<u8>,
    /// Worker ID
    pub worker_id: u32,
}

/// QuorumWaiter tracking ACKs qua P2P
pub struct QuorumWaiterP2p {
    /// Committee info để biết stake
    committee: Committee,
    /// Stake của node này
    own_stake: Stake,
    /// Nhận batch mới cần broadcast
    rx_batch: Receiver<QuorumWaiterMessage>,
    /// Gửi command đến P2P event loop
    tx_p2p_command: Sender<crate::req_res_protocol::WorkerP2pCommand>,
    /// Nhận ACK events từ P2P
    rx_p2p_event: Receiver<WorkerP2pEvent>,
    /// Forward batch đã có quorum đến Processor
    tx_processor: Sender<Vec<u8>>,
    /// Track pending batches
    pending_batches: HashMap<Vec<u8>, PendingBatch>,
}

#[derive(Debug)]
struct PendingBatch {
    batch_data: Vec<u8>,
    total_stake: Stake,
    acks_from: Vec<PublicKey>,
}

impl QuorumWaiterP2p {
    pub fn spawn(
        committee: Committee,
        own_stake: Stake,
        rx_batch: Receiver<QuorumWaiterMessage>,
        tx_p2p_command: Sender<crate::req_res_protocol::WorkerP2pCommand>,
        rx_p2p_event: Receiver<WorkerP2pEvent>,
        tx_processor: Sender<Vec<u8>>,
    ) {
        tokio::spawn(async move {
            Self {
                committee,
                own_stake,
                rx_batch,
                tx_p2p_command,
                rx_p2p_event,
                tx_processor,
                pending_batches: HashMap::new(),
            }
            .run()
            .await;
        });
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                // Nhận batch mới cần broadcast
                Some(msg) = self.rx_batch.recv() => {
                    self.handle_new_batch(msg).await;
                }
                
                // Nhận ACK event từ P2P
                Some(event) = self.rx_p2p_event.recv() => {
                    self.handle_p2p_event(event).await;
                }
            }
        }
    }

    async fn handle_new_batch(&mut self, msg: QuorumWaiterMessage) {
        info!("📦 [QuorumWaiter] Broadcasting batch {:?}", 
            hex::encode(&msg.batch_id[..8]));

        // Tạo pending entry với own stake
        self.pending_batches.insert(
            msg.batch_id.clone(),
            PendingBatch {
                batch_data: msg.batch.clone(),
                total_stake: self.own_stake,
                acks_from: Vec::new(),
            },
        );

        // Get target peers (cần có mapping PublicKey -> PeerId)
        let target_peers = self.get_peer_ids_for_workers();

        // Gửi command đến P2P để broadcast
        if let Err(e) = self.tx_p2p_command.send(
            crate::req_res_protocol::WorkerP2pCommand::BroadcastBatch {
                batch_data: msg.batch,
                batch_id: msg.batch_id.clone(),
                worker_id: msg.worker_id,
                target_peers,
            }
        ).await {
            warn!("❌ Failed to send broadcast command to P2P: {}", e);
        }
    }

    async fn handle_p2p_event(&mut self, event: WorkerP2pEvent) {
        match event {
            WorkerP2pEvent::BatchAck { batch_id, from_peer, responder_pubkey } => {
                info!("✅ [QuorumWaiter] Received ACK for batch {:?} from peer {}",
                    hex::encode(&batch_id[..8]), from_peer);

                // Deserialize public key
                let pubkey = match bincode::deserialize::<PublicKey>(&responder_pubkey) {
                    Ok(pk) => pk,
                    Err(e) => {
                        warn!("Failed to deserialize public key: {}", e);
                        return;
                    }
                };

                // Update stake
                if let Some(pending) = self.pending_batches.get_mut(&batch_id) {
                    let stake = self.committee.stake(&pubkey);
                    pending.total_stake += stake;
                    pending.acks_from.push(pubkey);

                    info!("📊 Batch {:?} now has stake: {} / {} (threshold: {})",
                        hex::encode(&batch_id[..8]),
                        pending.total_stake,
                        self.committee.total_stake(),
                        self.committee.quorum_threshold()
                    );

                    // Check if reached quorum
                    if pending.total_stake >= self.committee.quorum_threshold() {
                        info!("🎉 [QuorumWaiter] Batch {:?} reached quorum! Forwarding to Processor",
                            hex::encode(&batch_id[..8]));

                        let batch_data = pending.batch_data.clone();
                        self.pending_batches.remove(&batch_id);

                        // Forward to processor
                        if let Err(e) = self.tx_processor.send(batch_data).await {
                            warn!("❌ Failed to send batch to processor: {}", e);
                        }
                    }
                }
            }

            WorkerP2pEvent::BatchRequestFailed { batch_id, to_peer, error } => {
                warn!("❌ [QuorumWaiter] Request failed for batch {:?} to peer {}: {}",
                    hex::encode(&batch_id[..8]), to_peer, error);
                // TODO: Có thể implement retry logic ở đây
            }

            WorkerP2pEvent::BatchReceived { .. } => {
                // Được xử lý ở Worker receiver handler
            }
        }
    }

    fn get_peer_ids_for_workers(&self) -> Vec<libp2p::PeerId> {
        // TODO: Cần có mapping từ PublicKey -> PeerId
        // Có thể lưu trong một shared state hoặc discovery mechanism
        Vec::new()
    }
}
