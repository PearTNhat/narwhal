// Copyright(C) Facebook, Inc. and its affiliates.
use crate::quorum_waiter::QuorumWaiterMessage;
use crate::worker::WorkerMessage;
#[cfg(feature = "benchmark")]
use crypto::Digest;
use crypto::PublicKey;
#[cfg(feature = "benchmark")]
use ed25519_dalek::{Digest as _, Sha512};
#[cfg(feature = "benchmark")]
use log::info;
use network::ReliableSender;
#[cfg(feature = "benchmark")]
use std::convert::TryInto as _;
use std::net::SocketAddr;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{sleep, Duration, Instant};
use libp2p::PeerId;
use p2p::ReqResCommand;

#[cfg(test)]
#[path = "tests/batch_maker_tests.rs"]
pub mod batch_maker_tests;

pub type Transaction = Vec<u8>;
pub type Batch = Vec<Transaction>;

/// Assemble clients transactions into batches.
pub struct BatchMaker {
    /// The preferred batch size (in bytes).
    batch_size: usize,
    /// The maximum delay after which to seal the batch (in ms).
    max_batch_delay: u64,
    /// Channel to receive transactions from the network.
    rx_transaction: Receiver<Transaction>,
    /// Output channel to deliver sealed batches to the `QuorumWaiter`.
    tx_message: Sender<QuorumWaiterMessage>,
    /// The network addresses of the other workers that share our worker id.
    workers_addresses: Vec<(PublicKey, SocketAddr)>,
    /// Mapping from PublicKey to PeerId for P2P communication.
    peer_mapping: std::collections::HashMap<PublicKey, PeerId>,
    /// Channel to send P2P request-response commands.
    tx_req_res: Sender<ReqResCommand>,
    /// Request ID counter.
    request_counter: u64,
    /// Holds the current batch.
    current_batch: Batch,
    /// Holds the size of the current batch (in bytes).
    current_batch_size: usize,
}

impl BatchMaker {
    pub fn spawn(
        batch_size: usize,
        max_batch_delay: u64,
        rx_transaction: Receiver<Transaction>,
        tx_message: Sender<QuorumWaiterMessage>,
        workers_addresses: Vec<(PublicKey, SocketAddr)>,
        peer_mapping: std::collections::HashMap<PublicKey, PeerId>,
        tx_req_res: Sender<ReqResCommand>,
    ) {
        tokio::spawn(async move {
            Self {
                batch_size,
                max_batch_delay,
                rx_transaction,
                tx_message,
                workers_addresses,
                peer_mapping,
                tx_req_res,
                request_counter: 0,
                current_batch: Batch::with_capacity(batch_size * 2),
                current_batch_size: 0,
            }
            .run()
            .await;
        });
    }

    /// Main loop receiving incoming transactions and creating batches.
    async fn run(&mut self) {
        //Timer này đảm bảo rằng nếu batch chưa đủ to thì vẫn phải gửi đi sau một thời gian.
        let timer = sleep(Duration::from_millis(self.max_batch_delay));
        tokio::pin!(timer);

        loop {
            tokio::select! {
                // Assemble client transactions into batches of preset size.
                Some(transaction) = self.rx_transaction.recv() => {
                    self.current_batch_size += transaction.len();
                    self.current_batch.push(transaction);
                    if self.current_batch_size >= self.batch_size {
                        self.seal().await;
                        timer.as_mut().reset(Instant::now() + Duration::from_millis(self.max_batch_delay));
                    }
                },

                // If the timer triggers, seal the batch even if it contains few transactions.
                () = &mut timer => {
                    if !self.current_batch.is_empty() {
                        self.seal().await;
                    }
                    timer.as_mut().reset(Instant::now() + Duration::from_millis(self.max_batch_delay));
                }
            }

            // Give the change to schedule other tasks.
            tokio::task::yield_now().await;
        }
    }

    /// Seal and broadcast the current batch.
    async fn seal(&mut self) {
        #[cfg(feature = "benchmark")]
        let size = self.current_batch_size;

        // Look for sample txs (they all start with 0) and gather their txs id (the next 8 bytes).
        #[cfg(feature = "benchmark")]
        let tx_ids: Vec<_> = self
            .current_batch
            .iter()
            .filter(|tx| tx[0] == 0u8 && tx.len() > 8)
            .filter_map(|tx| tx[1..9].try_into().ok())
            .collect();

        // Serialize the batch.
        self.current_batch_size = 0;
        let batch: Vec<_> = self.current_batch.drain(..).collect();
        let message = WorkerMessage::Batch(batch);
        let serialized = bincode::serialize(&message).expect("Failed to serialize our own batch");

        #[cfg(feature = "benchmark")]
        {
            // NOTE: This is one extra hash that is only needed to print the following log entries.
            let digest = Digest(
                Sha512::digest(&serialized).as_slice()[..32]
                    .try_into()
                    .unwrap(),
            );

            for id in tx_ids {
                // NOTE: This log entry is used to compute performance.
                info!(
                    "Batch {:?} contains sample tx {}",
                    digest,
                    u64::from_be_bytes(id)
                );
            }

            // NOTE: This log entry is used to compute performance.
            info!("Batch {:?} contains {} B", digest, size);
        }

        // Broadcast the batch through P2P network.
        let names: Vec<_> = self.workers_addresses.iter().map(|(name, _)| *name).collect();
        let mut request_ids = Vec::new();
        
        for name in &names {
            if let Some(peer_id) = self.peer_mapping.get(name) {
                self.request_counter += 1;
                let request_id = self.request_counter;
                request_ids.push(request_id);
                
                let cmd = ReqResCommand::SendRequest {
                    request_id,
                    target_peer: *peer_id,
                    data: serialized.clone(),
                };
                
                if let Err(e) = self.tx_req_res.send(cmd).await {
                    log::warn!("Failed to send batch request to {}: {}", peer_id, e);
                }
            } else {
                log::warn!("No PeerId mapping found for worker: {:?}", name);
            }
        }

        // Send the batch through the deliver channel for further processing.
        self.tx_message
            .send(QuorumWaiterMessage {
                batch: serialized,
                request_ids,
                worker_names: names,
            })
            .await
            .expect("Failed to deliver batch");
    }
}
