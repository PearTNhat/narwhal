// Copyright(C) Facebook, Inc. and its affiliates.
use crate::processor::SerializedBatchMessage;
use config::{Committee, Stake};
use crypto::PublicKey;
use tokio::sync::mpsc::{Receiver, Sender};
use p2p::ReqResEvent;
use std::collections::{HashMap, HashSet};

#[cfg(test)]
#[path = "tests/quorum_waiter_tests.rs"]
pub mod quorum_waiter_tests;

#[derive(Debug)]
pub struct QuorumWaiterMessage {
    /// A serialized `WorkerMessage::Batch` message.
    pub batch: SerializedBatchMessage,
    /// Request IDs that were sent to workers.
    pub request_ids: Vec<u64>,
    /// Names of workers we sent to.
    pub worker_names: Vec<PublicKey>,
}

/// The QuorumWaiter waits for 2f authorities to acknowledge reception of a batch.
pub struct QuorumWaiter {
    /// The committee information.
    committee: Committee,
    /// The stake of this authority.
    stake: Stake,
    /// Input Channel to receive commands.
    rx_message: Receiver<QuorumWaiterMessage>,
    /// Channel to deliver batches for which we have enough acknowledgements.
    tx_batch: Sender<SerializedBatchMessage>,
    /// Channel to receive P2P ACK events.
    rx_req_res_event: Receiver<ReqResEvent>,
}

impl QuorumWaiter {
    /// Spawn a new QuorumWaiter.
    pub fn spawn(
        committee: Committee,
        stake: Stake,
        rx_message: Receiver<QuorumWaiterMessage>,
        tx_batch: Sender<Vec<u8>>,
        rx_req_res_event: Receiver<ReqResEvent>,
    ) {
        tokio::spawn(async move {
            Self {
                committee,
                stake,
                rx_message,
                tx_batch,
                rx_req_res_event,
            }
            .run()
            .await;
        });
    }

    // NOTE: Helper function không còn cần thiết với P2P implementation

    /// Main loop.
    async fn run(&mut self) {
        // Track pending batches: request_id -> (batch, expected_request_ids, worker_names, received_acks)
        let mut pending_batches: HashMap<u64, (SerializedBatchMessage, HashSet<u64>, Vec<PublicKey>, HashSet<u64>)> = HashMap::new();
        loop {
            tokio::select! {
                // Nhận batch mới từ BatchMaker
                Some(QuorumWaiterMessage { batch, request_ids, worker_names }) = self.rx_message.recv() => {
                    if request_ids.is_empty() {
                        // Không có worker nào để gửi, xử lý ngay
                        self.tx_batch
                            .send(batch)
                            .await
                            .expect("Failed to deliver batch");
                    } else {
                        // Lưu batch và chờ ACK
                        let batch_id = request_ids[0]; // Dùng request_id đầu tiên làm batch_id
                        let expected_ids: HashSet<u64> = request_ids.iter().cloned().collect();
                        pending_batches.insert(batch_id, (batch, expected_ids, worker_names, HashSet::new()));
                        log::info!("[QuorumWaiter] Waiting for ACKs for batch {}, expecting {} responses", batch_id, request_ids.len());
                    }
                }
                
                // Nhận ACK từ P2P
                Some(event) = self.rx_req_res_event.recv() => {
                    match event {
                        ReqResEvent::ResponseReceived { request_id, from_peer, success, message } => {
                            log::info!("[QuorumWaiter] Received ACK for request {} from {}: success={}, msg={}", 
                                request_id, from_peer, success, message);
                            
                            // Tìm batch_id tương ứng
                            let mut batch_to_forward = None;
                            let mut batch_id_to_remove = None;
                            
                            for (batch_id, (batch, expected_ids, worker_names, received_acks)) in pending_batches.iter_mut() {
                                if expected_ids.contains(&request_id) {
                                    received_acks.insert(request_id);
                                    
                                    // Tính tổng stake từ các worker đã ACK
                                    let mut total_stake = self.stake; // Bắt đầu với stake của chính mình
                                    for (i, req_id) in expected_ids.iter().enumerate() {
                                        if received_acks.contains(req_id) && i < worker_names.len() {
                                            total_stake += self.committee.stake(&worker_names[i]);
                                        }
                                    }
                                    
                                    log::info!("[QuorumWaiter] Batch {} - Total stake: {} / Quorum: {}", 
                                        batch_id, total_stake, self.committee.quorum_threshold());
                                    
                                    // Kiểm tra quorum
                                    if total_stake >= self.committee.quorum_threshold() {
                                        log::info!("[QuorumWaiter] ✅ Batch {} reached quorum! Forwarding to Processor", batch_id);
                                        batch_to_forward = Some(batch.clone());
                                        batch_id_to_remove = Some(*batch_id);
                                    }
                                    break;
                                }
                            }
                            
                            // Forward batch và xóa khỏi pending (sau khi thoát khỏi iter_mut)
                            if let Some(batch) = batch_to_forward {
                                self.tx_batch
                                    .send(batch)
                                    .await
                                    .expect("Failed to deliver batch");
                            }
                            if let Some(id) = batch_id_to_remove {
                                pending_batches.remove(&id);
                            }
                        }
                        ReqResEvent::RequestFailed { request_id, to_peer, error } => {
                            log::warn!("[QuorumWaiter] Request {} to {} failed: {}", request_id, to_peer, error);
                            // TODO: Implement retry logic or handle failure
                        }
                        _ => {}
                    }
                }
                
                else => {
                    log::warn!("QuorumWaiter channels closed");
                    break;
                }
            }
        }
    }
}
