// Copyright(C) Facebook, Inc. and its affiliates.
use crate::processor::SerializedBatchMessage;
use config::{Committee, Stake};
use crypto::PublicKey;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt as _;
use network::CancelHandler;
use tokio::sync::mpsc::{Receiver, Sender};

#[cfg(test)]
#[path = "tests/quorum_waiter_tests.rs"]
pub mod quorum_waiter_tests;

#[derive(Debug)]
pub struct QuorumWaiterMessage {
    /// A serialized `WorkerMessage::Batch` message.
    pub batch: SerializedBatchMessage,
    /// The cancel handlers to receive the acknowledgements of our broadcast.
    pub handlers: Vec<(PublicKey, CancelHandler)>,
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
}

impl QuorumWaiter {
    /// Spawn a new QuorumWaiter.
    pub fn spawn(
        committee: Committee,
        stake: Stake,
        rx_message: Receiver<QuorumWaiterMessage>,
        tx_batch: Sender<Vec<u8>>,
    ) {
        tokio::spawn(async move {
            Self {
                committee,
                stake,
                rx_message,
                tx_batch,
            }
            .run()
            .await;
        });
    }

    /// Helper function. It waits for a future to complete and then delivers a value.
    async fn waiter(wait_for: CancelHandler, deliver: Stake) -> Stake {
        let _ = wait_for.await;
        deliver
    }

    /// Main loop.
    async fn run(&mut self) {
        while let Some(QuorumWaiterMessage { batch, handlers }) = self.rx_message.recv().await {
            // 1. Tạo một tập hợp các "công việc" chờ đợi song song.
            let mut wait_for_quorum: FuturesUnordered<_> = handlers
                .into_iter()
                // 2. Với mỗi handler (đại diện cho một worker), tạo một "công việc" con.
                .map(|(name, handler)| {
                    // 3. Lấy ra "trọng số" (stake) của worker đó.
                    let stake = self.committee.stake(&name);
                    // 4. Tạo một future 'waiter' sẽ chờ handler hoàn thành rồi trả về stake.
                    Self::waiter(handler, stake)
                })
                .collect();

            // 5. Bắt đầu vòng lặp chờ đợi.
            let mut total_stake = self.stake; // Bắt đầu bằng stake của chính mình.
            while let Some(stake) = wait_for_quorum.next().await {
                // 6. `next().await` sẽ đợi cho đến khi BẤT KỲ worker nào gửi Ack về.
                //    Khi có Ack, nó trả về `stake` của worker đó.
                total_stake += stake; // 7. Cộng dồn "vote" vào tổng.
                if total_stake >= self.committee.quorum_threshold() {
                    // 8. Nếu tổng vote đạt ngưỡng, gửi batch đi xử lý tiếp.
                    self.tx_batch
                        .send(batch)
                        .await
                        .expect("Failed to deliver batch");
                    // 9. Thoát khỏi vòng lặp, không cần chờ thêm Ack nữa.
                    break;
                }
            }
        }
    }
}
